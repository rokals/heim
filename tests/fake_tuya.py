"""Faux appareil Tuya pour les tests : une prise avec puissance.

Il utilise les fonctions de la bibliothèque tinytuya (trames, chiffrement), donc il parle le
vrai protocole : si Heim s'entend avec lui, il s'entend avec les vrais appareils.

    python3 fake_tuya.py <ip> <version 3.3|3.4|3.5> <id> <clé> [annonce]
"""
import hmac
import json
import os
import socket
import struct
import sys
import threading
import time
from hashlib import sha256

from tinytuya.core import header as H
from tinytuya.core.crypto_helper import AESCipher
from tinytuya.core.message_helper import TuyaMessage, pack_message, parse_header, unpack_message
from tinytuya.core.udp_helper import udpkey

IP, VERSION, DEV_ID, KEY = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4].encode()
ANNOUNCE = len(sys.argv) > 5 and sys.argv[5] == "annonce"
VER = sys.argv[2].encode()
HEADER = VER + 12 * b"\x00"
dps = {"1": False, "19": 0}
lock = threading.Lock()


def send(conn, state, cmd, payload, seq):
    """Trame appareil -> client (avec code retour 0)."""
    key = state["key"]
    if VERSION >= 3.5:
        msg = TuyaMessage(seq, cmd, 0, payload, 0, True, H.PREFIX_6699_VALUE, True)
        conn.sendall(pack_message(msg, hmac_key=key))
        return
    if VERSION >= 3.4:
        body = AESCipher(key).encrypt(payload, False) if payload else b""
        msg = TuyaMessage(seq, cmd, 0, b"\x00\x00\x00\x00" + body, 0, True, H.PREFIX_55AA_VALUE, False)
        conn.sendall(pack_message(msg, hmac_key=key))
        return
    msg = TuyaMessage(seq, cmd, 0, b"\x00\x00\x00\x00" + payload, 0, True, H.PREFIX_55AA_VALUE, False)
    conn.sendall(pack_message(msg))


def enc33(obj, header=True):
    raw = AESCipher(KEY).encrypt(json.dumps(obj).encode(), False)
    return (HEADER + raw) if header else raw


def status_payload(changed):
    if VERSION >= 3.4:
        return HEADER + json.dumps({"protocol": 4, "t": int(time.time()), "data": {"dps": changed}}).encode()
    return enc33({"devId": DEV_ID, "dps": changed, "t": int(time.time())})


def query_payload():
    with lock:
        cur = dict(dps)
    if VERSION >= 3.4:
        return json.dumps({"dps": cur}).encode()
    return enc33({"devId": DEV_ID, "dps": cur}, header=False)


def decode(state, data):
    """Trame client -> appareil : (cmd, seq, charge en clair)."""
    head = parse_header(data)
    if VERSION >= 3.5:
        m = unpack_message(data, hmac_key=state["key"], header=head, no_retcode=True)
        payload = m.payload
    elif VERSION >= 3.4:
        m = unpack_message(data, hmac_key=state["key"], header=head, no_retcode=True)
        assert m.crc_good, "HMAC faux"
        payload = AESCipher(state["key"]).decrypt(m.payload, False, decode_text=False)
    else:
        m = unpack_message(data, header=head, no_retcode=True)
        assert m.crc_good, "CRC faux"
        payload = m.payload
        if payload.startswith(VER):
            payload = payload[15:]
        payload = AESCipher(KEY).decrypt(payload, False, decode_text=False)
    if payload.startswith(VER):
        payload = payload[15:]
    return m.cmd, m.seqno, payload, head.total_length


def serve(conn):
    state = {"key": KEY, "seq": 100}
    buf = b""

    def next_seq():
        state["seq"] += 1
        return state["seq"]

    while True:
        chunk = conn.recv(4096)
        if not chunk:
            return
        buf += chunk
        while len(buf) >= 16:
            try:
                head = parse_header(buf)
            except Exception:
                return
            total = head.total_length
            if len(buf) < total:
                break
            frame, buf = buf[:total], buf[total:]
            cmd, seq, payload, _ = decode(state, frame)
            if cmd == 3:  # début de négociation
                state["client_nonce"] = payload[:16]
                state["dev_nonce"] = os.urandom(16)
                resp = state["dev_nonce"] + hmac.new(KEY, payload[:16], sha256).digest()
                send(conn, state, 4, resp, next_seq())
            elif cmd == 5:  # fin : clé de session
                assert payload[:32] == hmac.new(KEY, state["dev_nonce"], sha256).digest(), "négociation fausse"
                xor = bytes(a ^ b for a, b in zip(state["client_nonce"], state["dev_nonce"]))
                if VERSION >= 3.5:
                    state["key"] = AESCipher(KEY).encrypt(xor, use_base64=False, pad=False, iv=state["client_nonce"][:12])[12:28]
                else:
                    state["key"] = AESCipher(KEY).encrypt(xor, False, pad=False)
            elif cmd in (10, 16):
                send(conn, state, cmd, query_payload(), next_seq())
            elif cmd in (7, 13):
                obj = json.loads(payload)
                changed = obj.get("dps") or obj.get("data", {}).get("dps", {})
                with lock:
                    dps.update(changed)
                    if "1" in changed:
                        dps["19"] = 853 if changed["1"] else 0
                        changed = dict(changed, **{"19": dps["19"]})
                send(conn, state, cmd, b"", next_seq())
                send(conn, state, 8, status_payload(changed), next_seq())
            elif cmd == 9:
                send(conn, state, 9, b"", next_seq())


def announce():
    """Annonce réseau toutes les secondes, comme un vrai appareil (port 6667, chiffrée)."""
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    info = json.dumps({"ip": IP, "gwId": DEV_ID, "active": 2, "ability": 0, "encrypt": True,
                       "productKey": "keyfake", "version": sys.argv[2]}).encode()
    while True:
        if VERSION >= 3.5:
            msg = TuyaMessage(1, 0x13, 0, info, 0, True, H.PREFIX_6699_VALUE, True)
            pkt = pack_message(msg, hmac_key=udpkey)
        else:
            body = b"\x00\x00\x00\x00" + AESCipher(udpkey).encrypt(info, False)
            pkt = pack_message(TuyaMessage(0, 0x13, 0, body, 0, True, H.PREFIX_55AA_VALUE, False))
        s.sendto(pkt, ("127.0.0.1", 6667))
        time.sleep(1)


if ANNOUNCE:
    threading.Thread(target=announce, daemon=True).start()
srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind((IP, 6668))
srv.listen(4)
print("prêt", flush=True)
while True:
    c, _ = srv.accept()
    threading.Thread(target=serve, args=(c,), daemon=True).start()
