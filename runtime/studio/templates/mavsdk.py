"""Read-only MAVSDK 4.x starter. Uses a dedicated configurable UDP endpoint."""
import argparse
import asyncio
from mavsdk.asyncio import ComponentType,Configuration,Mavsdk
from mavsdk.asyncio.plugins.telemetry import TelemetryAsync
async def main(endpoint):
    sdk=Mavsdk(Configuration.create_with_component_type(ComponentType.GROUND_STATION))
    await sdk.add_any_connection(endpoint)
    drone=await sdk.first_autopilot(30)
    if drone is None:raise RuntimeError('No autopilot heartbeat; check endpoint ownership and routing.')
    async for position in TelemetryAsync(drone).subscribe_position():print(position)
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--endpoint',default='udpin://0.0.0.0:14541')
    asyncio.run(main(parser.parse_args().endpoint))
