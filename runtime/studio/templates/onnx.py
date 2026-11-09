"""Run named, preprocessed NPZ tensors against an ONNX model; no remote code."""
import argparse
import json
from pathlib import Path
import time
import hashlib
import platform
import csv
def sha256(path):
    digest=hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda:stream.read(1024*1024),b''):digest.update(block)
    return digest.hexdigest()

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--model',required=True);parser.add_argument('--inputs',required=True);parser.add_argument('--output',default='artifacts')
    args=parser.parse_args()
    import numpy as np
    import onnxruntime as ort
    model=Path(args.model).resolve();inputs=Path(args.inputs).resolve();target=Path(args.output);target.mkdir(parents=True,exist_ok=True)
    session=ort.InferenceSession(str(model),providers=['CPUExecutionProvider'])
    with np.load(inputs,allow_pickle=False) as tensors:
        feed={item.name:tensors[item.name] for item in session.get_inputs()}
        targets=np.array(tensors['targets']) if 'targets' in tensors else None
        sample_ids=np.array(tensors['sample_ids']) if 'sample_ids' in tensors else None
    start=time.perf_counter();result=session.run(None,feed);latency=(time.perf_counter()-start)*1000
    np.savez(target/'outputs.npz',**{item.name:value for item,value in zip(session.get_outputs(),result)})
    predictions=None
    if targets is not None:
        scores=result[0]
        if scores.ndim!=2 or scores.shape[0]==0 or targets.shape!=(scores.shape[0],) or not np.isfinite(scores).all():raise ValueError('CSV export requires NxC classification scores and N matching targets.')
        ids=sample_ids if sample_ids is not None else np.arange(len(targets))
        if ids.shape!=targets.shape or len(set(map(str,ids)))!=len(ids):raise ValueError('Sample IDs must have one unique value per target.')
        predictions=target/'predictions.csv'
        with predictions.open('w',newline='',encoding='utf-8') as stream:
            writer=csv.writer(stream);writer.writerow(['sample_id','target','prediction','latency_ms'])
            writer.writerows((str(i),str(t),str(p),latency/len(targets)) for i,t,p in zip(ids,targets,scores.argmax(1)))
    (target/'run.json').write_text(json.dumps(dict(model=str(model),model_sha256=sha256(model),inputs=str(inputs),inputs_sha256=sha256(inputs),runtime=ort.__version__,numpy=np.__version__,python=platform.python_version(),latency_ms=latency,predictions=str(predictions) if predictions else None,predictions_sha256=sha256(predictions) if predictions else None,latency_note='One CPU batch measured; CSV per-sample latency is the amortized batch time.',provider='CPUExecutionProvider',outputs=[dict(name=i.name,shape=list(v.shape)) for i,v in zip(session.get_outputs(),result)]),indent=2))
    print('Inference outputs and latency written to',target)
if __name__=='__main__':main()
