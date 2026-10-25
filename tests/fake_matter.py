"""Faux serveur Matter (API WebSocket de python-matter-server) pour les tests.

    python3 fake_matter.py <port>
"""
import asyncio
import json
import sys

import websockets

NODE = {
    "node_id": 4, "date_commissioned": "2026-10-25T10:00:00", "last_interview": "2026-10-25T10:00:00",
    "interview_version": 6, "available": True, "is_bridge": False,
    "attributes": {
        "0/40/1": "IKEA of Sweden", "0/40/3": "KAJPLATS E27", "0/40/5": "",
        "1/29/0": [{"0": 268, "1": 4}],
        "1/6/0": False, "1/8/0": 100, "1/768/7": 370,
    },
}
clients = set()


async def broadcast(event, data):
    msg = json.dumps({"event": event, "data": data})
    for c in list(clients):
        try:
            await c.send(msg)
        except Exception:
            clients.discard(c)


async def handler(ws):
    await ws.send(json.dumps({"fabric_id": 1, "compressed_fabric_id": 123, "schema_version": 11,
                              "min_supported_schema_version": 9, "sdk_version": "2024.11.4",
                              "wifi_credentials_set": False, "thread_credentials_set": False,
                              "bluetooth_enabled": False}))
    async for raw in ws:
        msg = json.loads(raw)
        mid, cmd, args = msg.get("message_id"), msg.get("command"), msg.get("args", {})
        if cmd == "start_listening":
            clients.add(ws)
            await ws.send(json.dumps({"message_id": mid, "result": [NODE]}))
        elif cmd == "get_nodes":
            await ws.send(json.dumps({"message_id": mid, "result": [NODE]}))
        elif cmd == "device_command":
            ep, cl, name, payload = args["endpoint_id"], args["cluster_id"], args["command_name"], args.get("payload", {})
            changes = {}
            if cl == 6:
                changes[f"{ep}/6/0"] = name == "On"
            elif cl == 8 and name == "MoveToLevelWithOnOff":
                changes[f"{ep}/8/0"] = payload["level"]
                changes[f"{ep}/6/0"] = True
            elif cl == 768:
                changes[f"{ep}/768/7"] = payload["colorTemperatureMireds"]
            await ws.send(json.dumps({"message_id": mid, "result": None}))
            for path, value in changes.items():
                NODE["attributes"][path] = value
                await broadcast("attribute_updated", [args["node_id"], path, value])
        elif cmd == "commission_with_code":
            if args.get("code") == "MT:Y.K90AFN00KA0648G00":
                await ws.send(json.dumps({"message_id": mid, "result": NODE}))
            else:
                await ws.send(json.dumps({"message_id": mid, "error_code": 1, "details": "Commissioning failed"}))
        else:
            await ws.send(json.dumps({"message_id": mid, "error_code": 9, "details": "Invalid command"}))


async def main():
    async with websockets.serve(handler, "127.0.0.1", int(sys.argv[1])):
        print("prêt", flush=True)
        await asyncio.Future()

asyncio.run(main())
