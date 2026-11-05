"""ComfyUI motoru: gizli ComfyUI sürecini başlatır, iş gönderir, ilerlemeyi izler,
iş bitince kapatılır. PhotoCraft yalnız iş sırasında açar (sürekli çalışmaz).
"""
import json
import os
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid

import websocket  # websocket-client

KOK = os.path.dirname(os.path.abspath(__file__))
COMFY = os.path.join(KOK, "ComfyUI")
CALISMA = os.path.join(KOK, "calisma")
GIRDI = os.path.join(CALISMA, "comfy_in")
CIKTI = os.path.join(CALISMA, "comfy_out")
MODELLER = os.path.join(COMFY, "models")
LOG = os.path.join(KOK, "motor.log")
PORT = int(os.environ.get("EVREN_COMFY_PORT", "8195"))
ADRES = f"127.0.0.1:{PORT}"


# Mesaj dili (sunucu.py her istekte ayarlar).
DIL = "en"


def _t(en, tr):
    return tr if DIL == "tr" else en


class MotorHatasi(Exception):
    pass


class Durduruldu(Exception):
    pass


class Motor:
    def __init__(self):
        self.proc = None
        self._obj = None

    def calisiyor(self):
        return self.proc is not None and self.proc.poll() is None

    def baslat(self):
        if self.calisiyor():
            return
        if not os.path.isfile(os.path.join(COMFY, "main.py")):
            raise MotorHatasi(_t("ComfyUI not found (evren_ai\\ComfyUI); run evren_ai\\install.ps1", "ComfyUI bulunamadı (evren_ai\\ComfyUI); evren_ai\\install.ps1 betiğini çalıştırın"))
        os.makedirs(GIRDI, exist_ok=True)
        os.makedirs(CIKTI, exist_ok=True)
        _eski_motoru_kapat()
        py = sys.executable
        if py.lower().endswith("pythonw.exe"):
            py = py[:-11] + "python.exe"
        arg = [py, "-s", os.path.join(COMFY, "main.py"), "--listen", "127.0.0.1", "--port", str(PORT),
               "--disable-auto-launch", "--input-directory", GIRDI, "--output-directory", CIKTI,
               "--preview-method", "none", "--reserve-vram", "0.6"] + _hiz_ayarlari()
        env = dict(os.environ, PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
        log = open(LOG, "a", encoding="utf-8", errors="replace")
        log.write(f"\n===== {time.strftime('%Y-%m-%d %H:%M:%S')} motor başlıyor\n")
        log.flush()
        self.proc = subprocess.Popen(arg, cwd=COMFY, stdout=log, stderr=subprocess.STDOUT, env=env,
                                     creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self._obj = None
        t0 = time.time()
        while time.time() - t0 < 300:
            if not self.calisiyor():
                raise MotorHatasi(_t("The engine closed while starting (see evren_ai\\motor.log)", "Motor açılırken kapandı (evren_ai\\motor.log)"))
            try:
                self._get("/system_stats", zaman=3)
                return
            except Exception:
                time.sleep(0.7)
        raise MotorHatasi(_t("The engine did not start in time", "Motor zamanında açılmadı"))

    def durdur(self):
        p, self.proc = self.proc, None
        if p and p.poll() is None:
            try:
                p.terminate()
                p.wait(10)
            except Exception:
                try:
                    p.kill()
                except Exception:
                    pass

    def _get(self, yol, zaman=15):
        with urllib.request.urlopen(f"http://{ADRES}{yol}", timeout=zaman) as r:
            return json.loads(r.read().decode("utf-8"))

    def _post(self, yol, veri, zaman=30):
        istek = urllib.request.Request(f"http://{ADRES}{yol}", data=json.dumps(veri).encode("utf-8"),
                                       headers={"Content-Type": "application/json"}, method="POST")
        try:
            with urllib.request.urlopen(istek, timeout=zaman) as r:
                govde = r.read().decode("utf-8")
                return json.loads(govde) if govde.strip() else {}
        except urllib.error.HTTPError as e:
            raise MotorHatasi(_hata_ozet(e.read().decode("utf-8", "replace"))) from None

    def object_info(self):
        if self._obj is None:
            self._obj = self._get("/object_info", zaman=60)
        return self._obj

    def kes(self):
        try:
            self._post("/interrupt", {})
        except Exception:
            pass

    def calistir(self, workflow, ilerleme=None, iptal=None):
        """workflow'u çalıştırır; bitince {node_id: outputs} döner. ilerleme(olay, veri): "node" / "progress"."""
        self.baslat()
        cid = uuid.uuid4().hex
        ws = websocket.WebSocket()
        ws.connect(f"ws://{ADRES}/ws?clientId={cid}", timeout=10)
        ws.settimeout(1.0)
        try:
            cevap = self._post("/prompt", {"prompt": workflow, "client_id": cid})
            pid = cevap.get("prompt_id")
            if not pid:
                raise MotorHatasi(_hata_ozet(json.dumps(cevap)))
            kesildi = False
            while True:
                if iptal and iptal() and not kesildi:
                    kesildi = True
                    self.kes()
                if not self.calisiyor():
                    raise MotorHatasi(_t("The engine closed unexpectedly (maybe out of memory; see motor.log)", "Motor beklenmedik şekilde kapandı (bellek yetmemiş olabilir; motor.log)"))
                try:
                    m = ws.recv()
                except websocket.WebSocketTimeoutException:
                    continue
                if not isinstance(m, str):
                    continue
                try:
                    msg = json.loads(m)
                except ValueError:
                    continue
                t, d = msg.get("type"), msg.get("data") or {}
                if d.get("prompt_id") not in (None, pid):
                    continue
                if t == "executing":
                    if d.get("node") is None and d.get("prompt_id") == pid:
                        break
                    if ilerleme:
                        ilerleme("node", d.get("node"))
                elif t == "progress" and ilerleme:
                    ilerleme("progress", (d.get("value", 0), d.get("max", 1)))
                elif t == "execution_success":
                    break
                elif t == "execution_interrupted":
                    raise Durduruldu()
                elif t == "execution_error":
                    raise MotorHatasi(_cevir_hata(d.get("exception_message", "") or d.get("exception_type", "")))
            if kesildi:
                raise Durduruldu()
            gecmis = {}
            for _ in range(40):
                gecmis = self._get(f"/history/{pid}").get(pid, {})
                if gecmis.get("outputs") or gecmis.get("status", {}).get("completed") is not None:
                    break
                time.sleep(0.25)
            st = gecmis.get("status", {})
            if st.get("status_str") == "error":
                for ad, v in st.get("messages", []):
                    if ad == "execution_error":
                        raise MotorHatasi(_cevir_hata(v.get("exception_message", "")))
                raise MotorHatasi(_t("The job failed (see motor.log)", "İş hata verdi (motor.log)"))
            return gecmis.get("outputs", {})
        finally:
            try:
                ws.close()
            except Exception:
                pass


def _hiz_ayarlari():
    """SageAttention (kuruluysa) ve fp16 birikimli matris çarpımı. EVREN_HIZ=0 kapatır."""
    if os.environ.get("EVREN_HIZ") == "0":
        return []
    arg = ["--fast", "fp16_accumulation"]
    try:
        import importlib.util
        if importlib.util.find_spec("sageattention") and importlib.util.find_spec("triton"):
            arg.append("--use-sage-attention")
    except Exception:  # noqa: BLE001
        pass
    return arg


def _eski_motoru_kapat():
    """Önceki iş yarıda kalmışsa eski motor portu tutuyor olabilir: o süreci kapatır."""
    try:
        urllib.request.urlopen(f"http://{ADRES}/system_stats", timeout=1).close()
    except Exception:
        return
    komut = (f"Get-NetTCPConnection -LocalPort {PORT} -State Listen -ErrorAction SilentlyContinue | "
             "ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }")
    subprocess.run(["powershell", "-NoProfile", "-Command", komut], capture_output=True,
                   creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    time.sleep(1.5)


def cikti_yolu(img):
    return os.path.join(CIKTI, img.get("subfolder", ""), img["filename"])


def _hata_ozet(govde):
    try:
        j = json.loads(govde)
    except ValueError:
        return govde[:300]
    e = j.get("error") or {}
    parca = [e.get("message", "")] if isinstance(e, dict) else [str(e)]
    for nid, ne in (j.get("node_errors") or {}).items():
        for x in ne.get("errors", []):
            parca.append(f"{ne.get('class_type', nid)}: {x.get('message', '')} {x.get('details', '')}".strip())
    return " · ".join(p for p in parca if p)[:500] or _t("The engine rejected the job", "Motor işi kabul etmedi")


def _cevir_hata(s):
    k = s.lower()
    if "out of memory" in k or "allocation on device" in k:
        return _t("Out of graphics card memory. Try a smaller scale.", "Ekran kartı belleği yetmedi. Daha küçük bir büyütme oranı deneyin.")
    return s.strip()[:500] or _t("Unknown error", "Bilinmeyen hata")
