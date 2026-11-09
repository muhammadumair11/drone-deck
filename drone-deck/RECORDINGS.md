# rosbag2 + MCAP recordings — v0.4

Open **Recordings** in the sidebar, or **AI & data → Record & replay sensor data**.

1. Click **Check dependencies**. It checks ROS 2 Jazzy, rosbag2 and the MCAP storage plugin inside the configured WSL distribution.
2. Set the **capture ROS_DOMAIN_ID** to match your ROS publishers. The normal default is 0. Start your ROS sensor nodes or PX4-to-ROS bridge separately.
3. Click **Discover topics**, select the streams, or enter explicit absolute topic names separated by spaces or commas. For example, `/camera/image_raw /imu`. The recorder subscribes to these topics, including publishers that appear after capture starts.
4. Give the session a short name and a source/vehicle note. Leave **system time** selected unless your simulation actually publishes `/clock` and your workflow uses simulation time. With the ROS clock option, rosbag2 waits for the first clock message.
5. Click **Start MCAP recording**. Read the captured process logs for subscriptions and recording errors. A running process does not prove that sensor messages are arriving.
6. Click **Stop & finalize**. This sends SIGINT to the owned recorder, waits for its buffers and metadata to be written, and refreshes the library. Files split at 1 GiB, with a 32 MiB recorder cache.
7. Select a session to see message counts, duration, size, captured topic types, clock mode and source note. **Open selected folder** opens the WSL location in Windows File Explorer.

Recordings are stored on the Linux filesystem at `~/.local/share/drone-deck/recordings`, inside the selected WSL distribution. Each session contains `session.json` and a `bag` folder with `.mcap` files and rosbag2 `metadata.yaml`. This avoids writing high-rate recordings through the Windows-mounted project folder. Session names use UTC; metadata retains the clock mode and capture domain. There is no automatic deletion of recordings.

## Replay

Select a finalized, non-empty session, set a distinct replay domain (default **77**) and a speed between **0.1x and 4x**, then click **Replay selected recording**. Run your analysis nodes in the same WSL environment with the replay domain, for example:

```bash
source /opt/ros/jazzy/setup.bash
export ROS_DOMAIN_ID=77
export ROS_LOCALHOST_ONLY=1
# Start your analysis node here. Use use_sim_time:=true when it consumes playback /clock.
```

Replay uses local-only Fast DDS discovery and a domain different from both the current capture domain and the original session's domain. Known PX4 control-input topics and common command topics are filtered out. These are defaults for a separate analysis graph; keep live-vehicle control bridges out of that graph. Custom topic names cannot be classified automatically as sensor data versus commands. Replay does not connect to a flight controller or issue MAVLink flight commands itself.

**Stop replay** stops only the owned player. The global **Stop session** also requests recorder/player shutdown. Normal app close gracefully stops and finalizes owned recordings; allow time for the recorder to flush. An interrupted/failed recording is labeled for inspection rather than presented as complete. A finalized file may contain zero messages when no publishers were available; such a recording cannot be replayed through this page.

## PX4 prerequisite

PX4 SIH and MAVSDK do not automatically publish ROS 2 topics. PX4 sensor recording needs the appropriate PX4 ROS middleware bridge (normally the uXRCE-DDS client/agent) and matching `px4_msgs` definitions in the ROS workspace. Camera/LiDAR topics need their corresponding ROS publishers. This update installs the ROS recording stack; it does not install/configure the PX4 DDS agent or message workspace. Source your project's environment when starting its publishers. Topic schemas must match the publishers; the recorder does not convert incompatible messages.

The current recorder uses ROS 2 Jazzy under `/opt/ros/jazzy` in the distribution configured in Integrations. Changing distribution during an active recording/replay is blocked. Importing arbitrary external bag folders, pause/resume controls, a graphical timeline, dataset labeling, AI evaluation, and Rerun/Foxglove adapters remain planned capabilities.

## Verification

- Rust UI/state and responsive-layout tests, plus dependency-light Python worker tests.
- Actual ROS test: generated IMU publisher → MCAP recording → graceful finalization → metadata/file-signature checks → replay on another domain → subscriber receiving the recorded IMU values.
- The generated test data is temporary. No real drone or simulator is needed or started by the tests.

Official references: [rosbag2 Jazzy](https://github.com/ros2/rosbag2/tree/jazzy), [MCAP](https://mcap.dev/), [PX4 ROS 2](https://docs.px4.io/main/en/ros2/user_guide).
