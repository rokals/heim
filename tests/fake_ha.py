"""Faux Home Assistant (API WebSocket) pour les tests de reprise.

    python3 tests/fake_ha.py <port> <jeton>
"""
import asyncio, base64, io, json, sys, zipfile
import websockets

PORT, TOKEN = int(sys.argv[1]), sys.argv[2]
STOPPED = []

def backup():
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr("configuration.yaml", "mqtt:\n  server: mqtt://core-mosquitto\nserial:\n  port: /dev/ttyACM0\n")
        z.writestr("database.db", '{"id":1,"type":"Coordinator","ieeeAddr":"0x00"}\n{"id":2,"type":"Router","ieeeAddr":"0x0a"}\n')
    return json.dumps({"status": "ok", "data": {"zip": base64.b64encode(buf.getvalue()).decode()}})

async def handler(ws):
    await ws.send(json.dumps({"type": "auth_required", "ha_version": "2026.10.1"}))
    auth = json.loads(await ws.recv())
    if auth.get("access_token") != TOKEN:
        await ws.send(json.dumps({"type": "auth_invalid", "message": "bad"}))
        return
    await ws.send(json.dumps({"type": "auth_ok"}))
    subs = {}
    async for raw in ws:
        m = json.loads(raw)
        i, t = m["id"], m["type"]
        ok = lambda r=None: ws.send(json.dumps({"id": i, "type": "result", "success": True, "result": r}))
        if t == "mqtt/subscribe":
            subs[m["topic"]] = i
            await ok()
        elif t == "call_service" and m["domain"] == "mqtt":
            await ok({"context": {}})
            topic = m["service_data"]["topic"]
            if topic.endswith("/bridge/request/backup"):
                resp = topic.replace("/request/", "/response/")
                if resp in subs:
                    await ws.send(json.dumps({"id": subs[resp], "type": "event", "event": {"topic": resp, "payload": backup(), "qos": 0, "retain": False}}))
        elif t == "config/area_registry/list":
            await ok([{"area_id": "salon", "name": "Salon"}, {"area_id": "cuisine", "name": "Cuisine"}])
        elif t == "config/device_registry/list":
            await ok([
                {"id": "d1", "area_id": "salon", "identifiers": [["mqtt", "zigbee2mqtt_0x0A"]]},
                {"id": "d2", "area_id": "cuisine", "identifiers": [["hue", "abc"]]},
                {"id": "d3", "area_id": None, "identifiers": [["mqtt", "zigbee2mqtt_0x0b"]]},
            ])
        elif t == "supervisor/api" and m["endpoint"] == "/addons":
            await ok({"addons": [{"slug": "45df7312_zigbee2mqtt", "name": "Zigbee2MQTT", "state": "started"}]})
        elif t == "supervisor/api" and m["endpoint"].endswith("/stop"):
            STOPPED.append(m["endpoint"])
            await ok({})
        else:
            await ws.send(json.dumps({"id": i, "type": "result", "success": False, "error": {"code": "unknown_command", "message": "Unknown command."}}))

async def main():
    async with websockets.serve(handler, "127.0.0.1", PORT, max_size=None):
        print("fake ha prêt", flush=True)
        await asyncio.Future()

asyncio.run(main())
