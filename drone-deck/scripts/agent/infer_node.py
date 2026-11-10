"""Drone Deck inference node (template): image topic → ONNX model → JSON detections.

Started by `pipe.infer` on an isolated replay domain:

    python -u infer_node.py --model model.onnx --topic /camera/image_raw \
        --type sensor_msgs/msg/Image --output /drone_deck/detections

Each image is decoded, resized (nearest neighbour) to the model's first input, scaled to 0–1 for
float inputs, and run on the CPU. The node publishes one `std_msgs/String` per image whose data is
JSON: {stamp_ns, frame_id, topic, input, latency_ms, outputs: [{name, shape, dtype, values | top}]}.
Outputs are summarized generically (small tensors in full, otherwise the top-5 scores); decoding
boxes or classes is model-specific. To change pre- or post-processing for one project, copy this
file to `~/.local/share/drone-deck/projects/<project>/infer_node.py` and edit it there: runs that
name the project use that copy, and run.json records which file ran and its hash.

rclpy, onnxruntime and numpy are imported in `main` only, so the processing functions can be
tested without ROS.
"""
import argparse
import json
import math
import os
from pathlib import Path
import sys
import time

sys.path.insert(0, os.environ.get('DRONE_DECK_AGENT') or str(Path(__file__).resolve().parent))

import pipe  # noqa: E402

TOP_K = 5
FULL_LIMIT = 64


def dimension(value, fallback):
    return value if isinstance(value, int) and value > 0 else fallback


def layout(shape):
    """'nchw' or 'nhwc' for a 4-D image input; channels are the axis of size 1 or 3."""
    if len(shape) != 4:
        raise ValueError(f'Expected a 4-D image input, got shape {shape}.')
    if shape[1] in (1, 3):
        return 'nchw'
    if shape[3] in (1, 3):
        return 'nhwc'
    return 'nchw'


def preprocess(image, shape, numpy, float_input=True):
    """HxW or HxWxC uint8/uint16 image → batch tensor matching `shape` (dynamic dims allowed)."""
    order = layout(shape)
    if image.ndim == 2:
        image = image[:, :, None]
    channels = dimension(shape[1] if order == 'nchw' else shape[3], image.shape[2])
    height = dimension(shape[2] if order == 'nchw' else shape[1], image.shape[0])
    width = dimension(shape[3] if order == 'nchw' else shape[2], image.shape[1])
    if image.shape[2] == 4:
        image = image[:, :, :3]
    if channels == 1 and image.shape[2] == 3:
        image = image.mean(axis=2, keepdims=True).astype(image.dtype)
    elif channels == 3 and image.shape[2] == 1:
        image = numpy.repeat(image, 3, axis=2)
    rows = (numpy.arange(height) * image.shape[0] // height).clip(0, image.shape[0] - 1)
    cols = (numpy.arange(width) * image.shape[1] // width).clip(0, image.shape[1] - 1)
    image = image[rows][:, cols]
    if float_input:
        scale = 65535.0 if image.dtype == numpy.uint16 else 255.0
        tensor = image.astype(numpy.float32) / scale
    else:
        tensor = image
    if order == 'nchw':
        tensor = tensor.transpose(2, 0, 1)
    return numpy.ascontiguousarray(tensor[None])


def finite(value):
    """A rounded float, or None for NaN/inf (strict JSON readers such as Foxglove refuse them)."""
    number = float(value)
    return round(number, 6) if math.isfinite(number) else None


def summarize(names, outputs, numpy, top_k=TOP_K):
    """One JSON-ready summary per output tensor."""
    rows = []
    for name, value in zip(names, outputs):
        array = numpy.asarray(value)
        row = {'name': name, 'shape': list(array.shape), 'dtype': str(array.dtype)}
        flat = array.reshape(-1)
        if flat.size <= FULL_LIMIT:
            row['values'] = [finite(v) for v in flat]
        elif numpy.issubdtype(array.dtype, numpy.number):
            k = min(top_k, flat.size)
            scores = numpy.nan_to_num(flat.astype(numpy.float64), nan=-numpy.inf)
            best = numpy.argpartition(-scores, k - 1)[:k]
            best = best[numpy.argsort(-scores[best])]
            row['top'] = [[int(i), finite(flat[i])] for i in best]
        rows.append(row)
    return rows


def detection(stamp_ns, frame_id, topic, input_shape, latency_ms, summary):
    return json.dumps({'stamp_ns': stamp_ns, 'frame_id': frame_id, 'topic': topic,
                       'input': list(input_shape), 'latency_ms': round(latency_ms, 3),
                       'outputs': summary}, separators=(',', ':'), allow_nan=False)


def image_array(message, kind, numpy):
    """A received message → HxW(xC) array."""
    if kind == pipe.COMPRESSED:
        data = pipe.as_bytes(message.data)
        try:
            from PIL import Image
            import io
            return numpy.asarray(Image.open(io.BytesIO(data)).convert('RGB'))
        except ImportError:
            pass
        try:
            import cv2
        except ImportError:
            raise RuntimeError('CompressedImage input needs Pillow or OpenCV in the inference environment.') from None
        decoded = cv2.imdecode(numpy.frombuffer(data, numpy.uint8), cv2.IMREAD_COLOR)
        return decoded[:, :, ::-1]
    frame = pipe.decode_image(message.encoding, message.height, message.width, message.step,
                              message.data, bool(message.is_bigendian))
    return pipe.frame_array(frame, numpy)


def stamp_ns(header):
    return int(header.stamp.sec) * 1_000_000_000 + int(header.stamp.nanosec)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--model', required=True)
    parser.add_argument('--topic', required=True)
    parser.add_argument('--type', default=pipe.IMAGE, choices=(pipe.IMAGE, pipe.COMPRESSED))
    parser.add_argument('--output', default=pipe.DETECTIONS)
    args = parser.parse_args(argv)

    import numpy
    import onnxruntime
    import rclpy
    from rclpy.node import Node
    from rclpy.qos import qos_profile_sensor_data
    from std_msgs.msg import String
    if args.type == pipe.COMPRESSED:
        from sensor_msgs.msg import CompressedImage as Message
    else:
        from sensor_msgs.msg import Image as Message

    session = onnxruntime.InferenceSession(args.model, providers=['CPUExecutionProvider'])
    model_input = session.get_inputs()[0]
    float_input = 'float' in model_input.type
    names = [o.name for o in session.get_outputs()]
    print(f'Model {args.model}: input {model_input.name} {model_input.shape} {model_input.type}; '
          f'outputs {names}', flush=True)

    rclpy.init()
    node = Node('drone_deck_inference')
    publisher = node.create_publisher(String, args.output, 10)
    count = {'images': 0, 'errors': 0}

    def received(message):
        try:
            tensor = preprocess(image_array(message, args.type, numpy), model_input.shape, numpy, float_input)
            started = time.perf_counter()
            outputs = session.run(None, {model_input.name: tensor})
            latency = (time.perf_counter() - started) * 1000
            text = detection(stamp_ns(message.header), message.header.frame_id, args.topic,
                             tensor.shape, latency, summarize(names, outputs, numpy))
            publisher.publish(String(data=text))
            count['images'] += 1
            if count['images'] % 50 == 1:
                print(f'Processed {count["images"]} images ({latency:.1f} ms last).', flush=True)
        except Exception as error:  # noqa: BLE001 - one bad frame must not stop the run
            count['errors'] += 1
            if count['errors'] <= 5:
                print(f'Frame skipped: {error}', file=sys.stderr, flush=True)

    node.create_subscription(Message, args.topic, received, qos_profile_sensor_data)
    print(f'Ready: {args.topic} → {args.output}', flush=True)
    try:
        rclpy.spin(node)
    except (KeyboardInterrupt, Exception):  # noqa: BLE001 - SIGTERM/SIGINT end the node
        pass
    finally:
        print(f'Stopped after {count["images"]} images, {count["errors"]} skipped.', flush=True)
        node.destroy_node()
        if rclpy.ok():
            rclpy.shutdown()


if __name__ == '__main__':
    main()
