"""Read-only adapter for the user's installed MAVSDK 4.x environment."""
import asyncio
import json
from mavsdk.asyncio import ComponentType, Configuration, Mavsdk
from mavsdk.asyncio.plugins.telemetry import TelemetryAsync

def emit(**values):
    print('@telemetry ' + json.dumps({k: str(v) for k, v in values.items()}), flush=True)

async def main():
    sdk = Mavsdk(Configuration.create_with_component_type(ComponentType.GROUND_STATION))
    await sdk.add_any_connection('udpin://0.0.0.0:14540')
    drone = await sdk.first_autopilot(30)
    if drone is None:
        raise RuntimeError('No PX4 heartbeat within 30 seconds. Start a simulator first.')
    telemetry = TelemetryAsync(drone)
    emit(link='Connected')
    async def positions():
        async for p in telemetry.subscribe_position():
            emit(altitude=f'{p.relative_altitude_m:.1f} m', latitude=f'{p.latitude_deg:.7f}', longitude=f'{p.longitude_deg:.7f}')
    async def modes():
        async for m in telemetry.subscribe_flight_mode():
            emit(mode=getattr(m, 'name', str(m)))
    async def health():
        async for h in telemetry.subscribe_health():
            emit(position='Ready' if h.is_local_position_ok else 'Not ready')
    await asyncio.gather(positions(), modes(), health())

if __name__ == '__main__':
    asyncio.run(main())
