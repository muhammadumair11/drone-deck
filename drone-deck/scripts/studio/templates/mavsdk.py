"""Read-only MAVSDK 4.x starter: prints position updates and never arms or flies.

Drone Deck runs this project inside WSL. The default endpoint is PX4 SITL's own API stream:
PX4 SITL sends it to UDP 14540 inside WSL, so `udpin://0.0.0.0:14540` receives it there while
a simulation runs. Only one program can listen on that port. Drone Deck's scripts fan-out
(default 127.0.0.1:14540 on Windows) is for scripts running on Windows, which a WSL process
cannot receive. Pass --endpoint for any other source, such as a physical vehicle's own port.
"""
import argparse
import asyncio
from mavsdk.asyncio import ComponentType, Configuration, Mavsdk
from mavsdk.asyncio.plugins.telemetry import TelemetryAsync

DEFAULT_ENDPOINT = 'udpin://0.0.0.0:14540'


async def main(endpoint):
    sdk = Mavsdk(Configuration.create_with_component_type(ComponentType.GROUND_STATION))
    await sdk.add_any_connection(endpoint)
    drone = await sdk.first_autopilot(30)
    if drone is None:
        raise RuntimeError(f'No autopilot heartbeat on {endpoint}; check that a simulation runs '
                           'and that no other program listens on that port.')
    async for position in TelemetryAsync(drone).subscribe_position():
        print(position)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--endpoint', default=DEFAULT_ENDPOINT,
                        help='MAVSDK connection URL (default: PX4 SITL API stream in WSL)')
    asyncio.run(main(parser.parse_args().endpoint))
