"""Drone Deck re-simulation: the command player for ArduPilot (`arbiter.py --autopilot ardupilot`).

ArduPilot's DDS client takes no namespace, so the fresh vehicle is kept apart on a ROS domain of its
own (`--vehicle-domain`, its `DDS_DOMAIN_ID`), and this player joins both that domain and the run's:

- it publishes the recorded commands (`/ap/cmd_vel`, `/ap/cmd_gps_pose`, `/ap/joy`) on the vehicle's
  domain at their recorded times, stamped on the fresh clock;
- arm, mode switch and takeoff are services, which a recording cannot keep: the player calls them on
  the fresh vehicle where the recorded `/ap/status` changed (`derive_calls`);
- it relays the fresh vehicle's `/ap/*` reports to the run's domain, where your code runs unchanged,
  and serves `/ap/arm_motors`, `/ap/mode_switch`, `/ap/experimental/takeoff` and `/ap/prearm_check`
  there: the pre-arm check is passed on; the others, in shadow, are answered from what the fresh
  vehicle did, and after a takeover passed on;
- with `--tolerance`, your code's commands and calls are compared with the recording's
  (`takeover.py`), and once a difference lasts, your code steers for the rest of the run.

Timing follows `arbiter.py`, except that the fresh clock starts only once the fresh vehicle passes its
pre-arm checks: ArduPilot stamps messages with the time since boot until its clock is set, and a
just-booted vehicle cannot arm yet.

rclpy is imported in `main` only, so the derivation can be tested without ROS.
"""
import json
import threading
import time

import arbiter

STATUS = '/ap/status'
POSE = '/ap/pose/filtered'
ARM = '/ap/arm_motors'
MODE = '/ap/mode_switch'
TAKEOFF = '/ap/experimental/takeoff'
PREARM = '/ap/prearm_check'
SERVICE_TYPES = {ARM: 'ardupilot_msgs/srv/ArmMotors', MODE: 'ardupilot_msgs/srv/ModeSwitch',
                 TAKEOFF: 'ardupilot_msgs/srv/Takeoff', PREARM: 'std_srvs/srv/Trigger'}
COMMANDS = ('/ap/cmd_', '/ap/joy')
# ardupilot_msgs/Status vehicle types that take off in Guided through the takeoff service.
COPTERS = (2, 13)
COPTER_GUIDED = 4
TAKEOFF_SEARCH_NS = 30_000_000_000
MIN_TAKEOFF_M = 1.0
DEFAULT_TAKEOFF_M = 2.5
# How long a shadow answer waits for the fresh vehicle to do what was asked (s).
SHADOW_WAIT_S = {ARM: 2.0, MODE: 2.0, TAKEOFF: 3.0}
# Without a pre-arm check service (older ArduPilot), the clock starts at the first status this long
# after the vehicle first reported.
NO_PREARM_S = 30.0


def is_command(topic):
    return topic.startswith(COMMANDS)


def takeoff_altitude(poses, start_ns, end_ns):
    """The highest altitude (m above the origin) the pose reached in [start, end], rounded to 0.1 m
    and at least `MIN_TAKEOFF_M`; `DEFAULT_TAKEOFF_M` without a pose there."""
    heights = [m['pose']['position']['z'] for _, log_ns, m in poses
               if start_ns <= log_ns <= end_ns and isinstance(m, dict)
               and isinstance(m.get('pose'), dict) and isinstance(m['pose'].get('position'), dict)
               and isinstance(m['pose']['position'].get('z'), (int, float))]
    if not heights:
        return DEFAULT_TAKEOFF_M
    return round(max(max(heights), MIN_TAKEOFF_M), 1)


def derive_calls(statuses, poses=(), commands=()):
    """The arm, mode switch and takeoff calls a flight made, worked out from its /ap/status:
    [(log ns, service, request)], in order.

    - At the first report: its mode, and arming and takeoff when the vehicle was already armed or
      flying (a recording started mid-flight).
    - Later: each mode change and each arming while no failsafe is active (a failsafe's changes are
      the vehicle's own), and a takeoff where a copter in Guided starts flying, to the highest
      altitude its pose reached before the next recorded command (at most 30 s later).
    - Disarming is left to the vehicle, which disarms itself after landing.

    A change the vehicle made itself without a failsafe (a mission's end) also reads as a call."""
    rows = sorted((r for r in statuses if isinstance(r[2], dict)), key=lambda r: r[1])
    command_times = sorted(log_ns for topic, log_ns, _ in commands if is_command(topic))

    def takeoff(log_ns):
        later = [t for t in command_times if t > log_ns]
        end = min(later[0] if later else log_ns + TAKEOFF_SEARCH_NS, log_ns + TAKEOFF_SEARCH_NS)
        return (log_ns, TAKEOFF, {'alt': takeoff_altitude(poses, log_ns, end)})

    calls, last = [], None
    for _, log_ns, m in rows:
        armed, mode, flying = bool(m.get('armed')), m.get('mode'), bool(m.get('flying'))
        copter = m.get('vehicle_type') in COPTERS
        if last is None:
            if isinstance(mode, int):
                calls.append((log_ns, MODE, {'mode': mode}))
            if armed:
                calls.append((log_ns, ARM, {'arm': True}))
            if armed and flying and copter:
                calls.append(takeoff(log_ns))
        elif not m.get('failsafe'):
            if isinstance(mode, int) and mode != last['mode']:
                calls.append((log_ns, MODE, {'mode': mode}))
            if armed and not last['armed']:
                calls.append((log_ns, ARM, {'arm': True}))
            if flying and not last['flying'] and armed and copter and mode == COPTER_GUIDED:
                calls.append(takeoff(log_ns))
        last = {'armed': armed, 'mode': mode, 'flying': flying}
    return calls


def schedule_for(recorded, reports, derive=True):
    """The replay schedule of a recorded ArduPilot flight: its commands and, with `derive`, the
    calls worked out from its reports in the clock base the commands were sent in
    (`arbiter.build_schedule`). A flight imported from its log brings its calls with its commands."""
    pairs = [(log_ns, s) for _, log_ns, m in reports if (s := arbiter.stamp_us(m)) is not None]
    base = arbiter.final_base(pairs)
    if base is None:
        raise ValueError('The recording has no ArduPilot time stamps to time its commands by.')
    statuses = [r for r in reports if r[0] == STATUS and r[1] >= base]
    if not statuses:
        raise ValueError('The recording has no /ap/status to start the replay from.')
    poses = [r for r in reports if r[0] == POSE]
    calls = [(service, log_ns, request)
             for log_ns, service, request in derive_calls(statuses, poses, recorded)] if derive else []
    schedule, early = arbiter.build_schedule(recorded + calls, reports)
    return schedule, early, len(calls)


def shadow_answer(service, request, fresh):
    """What your code's call returns before a takeover: whether the fresh vehicle is now as asked
    (`fresh`: its last status, a dict with armed, mode, flying). Returns the response fields."""
    if service == ARM:
        return {'result': bool(fresh.get('armed')) == bool(request.get('arm'))}
    if service == MODE:
        return {'status': fresh.get('mode') == request.get('mode'), 'curr_mode': int(fresh.get('mode') or 0)}
    if service == TAKEOFF:
        return {'status': bool(fresh.get('flying'))}
    return {}


def qos_kind(topic):
    """How ArduPilot publishes `topic`: `latched` (reliable, transient local), `reliable` or
    `stream` (best effort)."""
    if topic in (STATUS, '/ap/tf_static', '/ap/goal_lla'):
        return 'latched'
    if topic in ('/ap/time', '/ap/clock'):
        return 'reliable'
    return 'stream'


# -- ROS ---------------------------------------------------------------------------------------
def main(args):
    """Runs the player; returns the exit code (0 done, 2 unusable recording, 3 no ready vehicle)."""
    import os
    import pipe
    from takeover import Takeover

    import rclpy
    from rclpy.callback_groups import ReentrantCallbackGroup
    from rclpy.executors import MultiThreadedExecutor, SingleThreadedExecutor
    from rclpy.qos import QoSProfile, ReliabilityPolicy, HistoryPolicy, DurabilityPolicy
    from rosidl_runtime_py.set_message import set_message_fields
    from rosidl_runtime_py.utilities import get_message, get_service

    args.out.mkdir(parents=True, exist_ok=True)
    domain = int(os.environ.get('ROS_DOMAIN_ID') or 0)
    if args.vehicle_domain is None or args.vehicle_domain == domain:
        arbiter.write_status(args.out, {'commands': 0, 'sent': 0, 'anchored': False,
                                        'error': 'The fresh ArduPilot needs a ROS domain of its own.'})
        return 2
    recipe = json.loads((args.recording / 'recipe.json').read_text())
    types, recorded, reports = arbiter.read_recording(args.recording, recipe)
    try:
        schedule, early, derived = schedule_for(recorded, reports,
                                                derive=recipe.get('source') not in arbiter.FROM_LOG)
    except ValueError as error:
        arbiter.write_status(args.out, {'commands': 0, 'sent': 0, 'anchored': False, 'error': str(error)})
        return 2
    print(json.dumps({'kind': 'arbiter', 'commands': len(schedule), 'before_anchor': early,
                      'derived_calls': derived}), flush=True)

    profiles = {
        'latched': QoSProfile(reliability=ReliabilityPolicy.RELIABLE, durability=DurabilityPolicy.TRANSIENT_LOCAL,
                              history=HistoryPolicy.KEEP_LAST, depth=1),
        'reliable': QoSProfile(reliability=ReliabilityPolicy.RELIABLE, durability=DurabilityPolicy.VOLATILE,
                               history=HistoryPolicy.KEEP_LAST, depth=20),
        'stream': QoSProfile(reliability=ReliabilityPolicy.BEST_EFFORT, durability=DurabilityPolicy.VOLATILE,
                             history=HistoryPolicy.KEEP_LAST, depth=5)}
    command_qos = QoSProfile(reliability=ReliabilityPolicy.RELIABLE, durability=DurabilityPolicy.VOLATILE,
                             history=HistoryPolicy.KEEP_LAST, depth=5)

    rclpy.init()
    vehicle_context = rclpy.Context()
    rclpy.init(context=vehicle_context, domain_id=args.vehicle_domain)
    code = rclpy.create_node('drone_deck_arbiter')
    vehicle = rclpy.create_node('drone_deck_arbiter_vehicle', context=vehicle_context)
    vehicle_exec = SingleThreadedExecutor(context=vehicle_context)
    vehicle_exec.add_node(vehicle)
    code_exec = MultiThreadedExecutor(num_threads=4)
    code_exec.add_node(code)
    stop = threading.Event()

    def spin_vehicle():
        while not stop.is_set() and vehicle_context.ok():
            vehicle_exec.spin_once(timeout_sec=0.05)
    spinner = threading.Thread(target=spin_vehicle, daemon=True)
    spinner.start()

    lock = threading.RLock()
    clock, player = arbiter.FreshClock(), arbiter.Player(schedule)
    judge = Takeover(args.tolerance, args.floor, int(args.hold * 1e6))
    fresh = {'armed': False, 'mode': None, 'flying': False, 'failsafe': []}
    ready = {'at': None, 'first_status': None, 'asked': 0.0, 'pending': None}
    classes = {topic: get_message(kind) for topic, kind in types.items() if kind}
    publishers = {topic: vehicle.create_publisher(classes[topic], topic, command_qos)
                  for topic in {t for _, t, _ in schedule} if topic in classes}
    clients = {name: vehicle.create_client(get_service(kind), name) for name, kind in SERVICE_TYPES.items()}
    log = (args.out / 'commands.jsonl').open('a')
    relays, seen = [], set()
    missed = []  # recorded calls the fresh vehicle had no service for

    def report(error=None):
        value = arbiter.status(player, clock, time.monotonic(), early, error, judge)
        value['derived_calls'] = derived
        value['calls_not_reached'] = len(missed)
        return value

    def announce(taken):
        print(json.dumps({'kind': 'arbiter', 'takeover': taken}), flush=True)
        log.write(json.dumps({'t': taken['t'], 'takeover': taken}) + '\n')
        arbiter.write_status(args.out, report())

    def observe_clock(message):
        stamp = arbiter.stamp_us(pipe.plain(message))
        if stamp is not None and ready['at'] is not None:
            with lock:
                clock.observe(stamp, time.monotonic())

    def relay(topic, kind):
        cls = get_message(kind)
        profile = profiles[qos_kind(topic)]
        out = code.create_publisher(cls, topic, profile)

        def forward(message):
            if topic == STATUS:
                plain = pipe.plain(message)
                with lock:
                    fresh.update(armed=bool(plain.get('armed')), mode=plain.get('mode'),
                                 flying=bool(plain.get('flying')), failsafe=plain.get('failsafe') or [])
                    if ready['first_status'] is None:
                        ready['first_status'] = time.monotonic()
            if topic in (STATUS, '/ap/time'):
                observe_clock(message)
            out.publish(message)
        relays.append(vehicle.create_subscription(cls, topic, forward, profile))

    def call(service, request_fields, wait_s=None):
        """Calls `service` on the fresh vehicle; the response, or None (timeout, no service)."""
        client = clients[service]
        if not client.service_is_ready():
            return None
        request = client.srv_type.Request()
        set_message_fields(request, request_fields)
        future = client.call_async(request)
        if wait_s is None:
            return future
        done = threading.Event()
        future.add_done_callback(lambda _: done.set())
        return future.result() if done.wait(wait_s) else None

    def proxy(service):
        kind = get_service(SERVICE_TYPES[service])

        def serve(request, response):
            now = clock.now_us(time.monotonic())
            fields = pipe.plain(request)
            with lock:
                taken = judge.taken
                if not taken and service != PREARM and now is not None:
                    judge.observe(service, fields, now)
            if taken or service == PREARM:
                answer = call(service, fields, wait_s=5.0)
                if answer is not None:
                    for name in answer.get_fields_and_field_types():
                        setattr(response, name, getattr(answer, name))
                if taken and now is not None and clock.anchor is not None:
                    with lock:
                        log.write(arbiter.log_line(now - clock.anchor, service, 'your code'))
                return response
            until = time.monotonic() + SHADOW_WAIT_S.get(service, 0.0)
            while True:
                with lock:
                    answer = shadow_answer(service, fields, fresh)
                if answer.get('result', answer.get('status')) or time.monotonic() >= until:
                    break
                time.sleep(0.05)
            set_message_fields(response, answer)
            return response
        return code.create_service(kind, service, serve, callback_group=ReentrantCallbackGroup())

    services = [proxy(name) for name in SERVICE_TYPES]

    def listen_to(topic, kind):
        cls = get_message(kind)
        out = publishers.get(topic) or vehicle.create_publisher(cls, topic, command_qos)
        publishers.setdefault(topic, out)

        def heard(message):
            now = clock.now_us(time.monotonic())
            with lock:
                if judge.taken:
                    out.publish(message)
                    if now is not None and clock.anchor is not None:
                        log.write(arbiter.log_line(now - clock.anchor, topic, 'your code'))
                elif now is not None:
                    judge.observe(topic, pipe.plain(message), now)
        relays.append(code.create_subscription(cls, topic, heard, profiles['stream']))

    def check_ready(mono):
        """Starts the fresh clock once the vehicle passes its pre-arm checks."""
        if ready['at'] is not None:
            return
        pending = ready['pending']
        if pending is not None and pending.done():
            ready['pending'] = None
            result = pending.result()
            if result is not None and result.success:
                ready['at'] = mono
                return
        if pending is None and mono - ready['asked'] >= 1.0 and clients[PREARM].service_is_ready():
            ready['asked'] = mono
            ready['pending'] = call(PREARM, {})
        first = ready['first_status']
        if not clients[PREARM].service_is_ready() and first is not None and mono - first >= NO_PREARM_S:
            ready['at'] = mono  # no pre-arm check service: give the vehicle time to settle instead

    marks = {'scan': 0.0, 'status': 0.0}

    def tick():
        mono = time.monotonic()
        check_ready(mono)
        if mono - marks['scan'] >= 1.0:
            marks['scan'] = mono
            for name, kinds in vehicle.get_topic_names_and_types():
                if name in seen or not kinds or not name.startswith('/ap/') or is_command(name):
                    continue
                seen.add(name)
                relay(name, kinds[0])
            for name, kinds in code.get_topic_names_and_types():
                key = 'code:' + name
                if key in seen or not kinds or not is_command(name):
                    continue
                if judge.watching or judge.taken:
                    seen.add(key)
                    listen_to(name, kinds[0])
        with lock:
            if mono - marks['status'] >= 0.5:
                marks['status'] = mono
                arbiter.write_status(args.out, report())
            for topic, message, offset in player.due(clock, mono):
                if judge.taken:
                    continue  # one way: the recording never steers again
                if topic in SERVICE_TYPES:
                    skip = (topic == MODE and fresh.get('mode') == message.get('mode')) or \
                           (topic == ARM and bool(fresh.get('armed')) == bool(message.get('arm')))
                    if not skip and call(topic, message) is None:
                        missed.append(topic)
                else:
                    typed = classes[topic]()
                    set_message_fields(typed, message)
                    publishers[topic].publish(typed)
                log.write(arbiter.log_line(offset, topic))
                taken = judge.judge(topic, message, clock.anchor + offset, clock.anchor)
                if taken:
                    announce(taken)
            now = clock.now_us(mono)
            if now is not None and clock.anchor is not None:
                taken = judge.tick(now, clock.anchor)
                if taken:
                    announce(taken)
            if player.done:
                log.flush()

    code.create_timer(0.005, tick)
    started, status = time.monotonic(), 0
    try:
        while rclpy.ok() and not player.done:
            code_exec.spin_once(timeout_sec=0.05)
            if clock.anchor is None and time.monotonic() - started > args.wait:
                error = (f'The fresh ArduPilot did not pass its pre-arm checks on ROS domain {args.vehicle_domain} within '
                         f'{args.wait:g} s. Check that its SITL has the DDS client (Setup: ArduPilot ROS 2 (DDS) support) '
                         'and that the DDS agent on UDP 2019 runs.')
                arbiter.write_status(args.out, report(error))
                status = 3
                break
        if status == 0:
            print(json.dumps({'kind': 'arbiter', 'done': True}), flush=True)
            until = time.monotonic() + args.tail
            while rclpy.ok() and time.monotonic() < until:  # the fresh vehicle settles
                code_exec.spin_once(timeout_sec=0.1)
            arbiter.write_status(args.out, report())
    except KeyboardInterrupt:
        arbiter.write_status(args.out, report())
    finally:
        # Callbacks on either domain may still run: let them finish before the log closes.
        stop.set()
        spinner.join(timeout=2.0)
        code_exec.shutdown(timeout_sec=2.0)
        vehicle_exec.shutdown(timeout_sec=2.0)
        log.close()
        for service in services:
            code.destroy_service(service)
        code.destroy_node()
        vehicle.destroy_node()
        if vehicle_context.ok():
            rclpy.shutdown(context=vehicle_context)
        if rclpy.ok():
            rclpy.shutdown()
    return status
