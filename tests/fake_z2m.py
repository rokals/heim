"""Faux Zigbee2MQTT pour les tests : publie une liste d'appareils et obéit aux /set comme le vrai.

    python3 tests/fake_z2m.py <port du broker>
"""
import json, sys, time
import paho.mqtt.client as mqtt

BASE = "zigbee2mqtt"
DEVICES = [
    {"type": "Coordinator", "friendly_name": "Coordinator", "ieee_address": "0x00"},
    {"type": "Router", "friendly_name": "Salon/Plafonnier", "ieee_address": "0x01",
     "definition": {"model": "LED1545G12", "vendor": "IKEA", "exposes": [
         {"type": "light", "features": [
             {"type": "binary", "name": "state", "property": "state", "value_on": "ON", "value_off": "OFF", "access": 7},
             {"type": "numeric", "name": "brightness", "property": "brightness", "value_min": 0, "value_max": 254, "access": 7}]},
         {"type": "numeric", "name": "linkquality", "property": "linkquality", "access": 1}]}},
    {"type": "Router", "friendly_name": "Prise cafe", "ieee_address": "0x02", "power_source": "Mains (single phase)", "software_build_id": "0122052017",
     "definition": {"model": "TS011F", "vendor": "TuYa", "description": "Smart plug (with power monitoring)", "exposes": [
         {"type": "switch", "features": [{"type": "binary", "name": "state", "property": "state", "value_on": "ON", "value_off": "OFF", "access": 7}]},
         {"type": "numeric", "name": "power", "property": "power", "unit": "W", "access": 5},
         {"type": "enum", "name": "power_outage_memory", "property": "power_outage_memory", "access": 7, "values": ["on", "off", "restore"], "category": "config", "label": "Power outage memory"},
         {"type": "enum", "name": "indicator_mode", "property": "indicator_mode", "access": 7, "values": ["off", "off/on", "on/off", "on"], "category": "config"},
         {"type": "binary", "name": "child_lock", "property": "child_lock", "access": 7, "value_on": "LOCK", "value_off": "UNLOCK", "category": "config"}]}},
    {"type": "Router", "friendly_name": "Salon/Lampe couleur", "ieee_address": "0x05", "power_source": "Mains (single phase)",
     "definition": {"model": "LCA001", "vendor": "Philips", "description": "Hue White and Color Ambiance E27", "exposes": [
         {"type": "light", "features": [
             {"type": "binary", "name": "state", "property": "state", "value_on": "ON", "value_off": "OFF", "value_toggle": "TOGGLE", "access": 7},
             {"type": "numeric", "name": "brightness", "property": "brightness", "value_min": 0, "value_max": 254, "access": 7},
             {"type": "numeric", "name": "color_temp", "property": "color_temp", "value_min": 153, "value_max": 500, "access": 7, "unit": "mired"},
             {"type": "composite", "name": "color_xy", "property": "color", "access": 7, "features": [
                 {"type": "numeric", "name": "x", "property": "x", "access": 7}, {"type": "numeric", "name": "y", "property": "y", "access": 7}]}]},
         {"type": "enum", "name": "effect", "property": "effect", "access": 2, "values": ["blink", "breathe", "okay", "channel_change", "candle", "finish_effect", "stop_effect"]},
         {"type": "enum", "name": "power_on_behavior", "label": "Power-on behavior", "property": "power_on_behavior", "access": 7, "values": ["off", "on", "toggle", "previous"], "category": "config", "description": "Controls the behavior when the device is powered on after power loss"},
         {"type": "numeric", "name": "color_temp_startup", "property": "color_temp_startup", "access": 7, "value_min": 153, "value_max": 500, "category": "config",
          "presets": [{"name": "coolest", "value": 153}, {"name": "warmest", "value": 500}, {"name": "previous", "value": 65535}]},
         {"type": "composite", "name": "level_config", "property": "level_config", "access": 7, "category": "config", "features": [
             {"type": "numeric", "name": "on_level", "property": "on_level", "value_min": 1, "value_max": 254, "access": 7}]},
         {"type": "numeric", "name": "linkquality", "property": "linkquality", "access": 1}]}},
    {"type": "EndDevice", "friendly_name": "Porte entree", "ieee_address": "0x03",
     "definition": {"model": "MCCGQ11LM", "vendor": "Aqara", "exposes": [
         {"type": "binary", "name": "contact", "property": "contact", "value_on": False, "value_off": True, "access": 1},
         {"type": "numeric", "name": "battery", "property": "battery", "unit": "%", "access": 1}]}},
]
SIREN = {"type": "Router", "friendly_name": "Sirene garage", "ieee_address": "0x10",
     "definition": {"model": "HS2WD-E", "vendor": "Heiman", "description": "Smart siren", "exposes": [
         {"type": "composite", "name": "warning", "property": "warning", "access": 2, "features": [
             {"type": "enum", "name": "mode", "property": "mode", "access": 2, "values": ["stop", "burglar"]}]},
         {"type": "numeric", "name": "battery", "property": "battery", "unit": "%", "access": 1}]}}
state = {
    "Salon/Plafonnier": {"state": "OFF", "brightness": 120, "linkquality": 80},
    "Prise cafe": {"state": "OFF", "power": 0, "power_outage_memory": "off", "indicator_mode": "off/on", "child_lock": "UNLOCK"},
    "Salon/Lampe couleur": {"state": "ON", "brightness": 180, "color_temp": 370, "color_mode": "xy", "color": {"x": 0.55, "y": 0.38}, "power_on_behavior": "on", "level_config": {"on_level": 254}, "linkquality": 120,
        "update": {"state": "available", "installed_version": 16786690, "latest_version": 16786945}},
}
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 1883
log = sys.stderr

def on_connect(c, *_):
    c.subscribe(f"{BASE}/+/set")
    c.subscribe(f"{BASE}/+/+/set")
    c.subscribe(f"{BASE}/+/get")
    c.subscribe(f"{BASE}/bridge/request/#")
    c.publish(f"{BASE}/bridge/info", json.dumps({"version": "2.1.0", "permit_join": False}), retain=True)
    c.subscribe(f"{BASE}/+/+/get")
    c.publish(f"{BASE}/bridge/state", json.dumps({"state": "online"}), retain=True)
    c.publish(f"{BASE}/bridge/devices", json.dumps(DEVICES), retain=True)
    for name in state:
        c.publish(f"{BASE}/{name}/availability", json.dumps({"state": "online"}), retain=True)
    log.write("fake z2m prêt\n")

import threading
def join_later(c):
    time.sleep(2)
    c.publish(f"{BASE}/bridge/event", json.dumps({"type": "device_joined", "data": {"friendly_name": "0x10", "ieee_address": "0x10"}}))
    time.sleep(0.5)
    c.publish(f"{BASE}/bridge/event", json.dumps({"type": "device_interview", "data": {"friendly_name": "0x10", "status": "started"}}))
    time.sleep(1)
    DEVICES.append(SIREN)
    c.publish(f"{BASE}/bridge/devices", json.dumps(DEVICES), retain=True)
    c.publish(f"{BASE}/bridge/event", json.dumps({"type": "device_interview", "data": {"friendly_name": "Sirene garage", "status": "successful", "definition": SIREN["definition"]}}))
    c.publish(f"{BASE}/Sirene garage/availability", json.dumps({"state": "online"}), retain=True)
    c.publish(f"{BASE}/Sirene garage", json.dumps({"battery": 100}))

def on_message(c, _u, msg):
    if msg.topic.startswith(f"{BASE}/bridge/request/"):
        log.write(f"recu {msg.topic} {msg.payload.decode()}\n")
        if msg.topic.endswith("permit_join"):
            t = json.loads(msg.payload).get("time", 0)
            info = {"version": "2.1.0", "permit_join": t > 0}
            if t > 0:
                info["permit_join_end"] = int(time.time() * 1000) + t * 1000
                if not any(d["ieee_address"] == "0x10" for d in DEVICES):
                    threading.Thread(target=join_later, args=(c,), daemon=True).start()
            c.publish(f"{BASE}/bridge/info", json.dumps(info), retain=True)
            c.publish(f"{BASE}/bridge/response/permit_join", json.dumps({"status": "ok", "data": {"time": t}}))
        return
    parts = msg.topic[len(BASE) + 1:].rsplit("/", 1)
    name, verb = parts[0], parts[1]
    log.write(f"recu {msg.topic} {msg.payload.decode()}\n")
    if name not in state:
        return
    if verb == "set":
        payload = json.loads(msg.payload)
        state[name].update(payload)
        if name == "Prise cafe":
            state[name]["power"] = 850 if state[name]["state"] == "ON" else 0
        if "color" in payload and "hex" in payload.get("color", {}):
            h = payload["color"]["hex"].lstrip("#")
            r, g, b = (int(h[i:i+2], 16) / 255 for i in (0, 2, 4))
            lin = lambda v: ((v + 0.055) / 1.055) ** 2.4 if v > 0.04045 else v / 12.92
            r, g, b = lin(r), lin(g), lin(b)
            X = r*0.664511 + g*0.154324 + b*0.162028; Y = r*0.283881 + g*0.668433 + b*0.047685; Z = r*0.000088 + g*0.072310 + b*0.986039
            s_ = X + Y + Z
            state[name]["color"] = {"x": round(X/s_, 4), "y": round(Y/s_, 4)}
            state[name]["color_mode"] = "xy"
        state[name].pop("transition", None)
    time.sleep(0.05)
    c.publish(f"{BASE}/{name}", json.dumps(state[name]))

c = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="fake-z2m")
c.on_connect = on_connect
c.on_message = on_message
c.connect("127.0.0.1", PORT)
time.sleep(0.5)
c.loop_start()
time.sleep(1.5)
# La porte s'ouvre puis se ferme.
c.publish(f"{BASE}/Porte entree", json.dumps({"contact": False, "battery": 91}))
time.sleep(0.5)
c.publish(f"{BASE}/Porte entree", json.dumps({"contact": True, "battery": 91}))
while True:
    time.sleep(1)
