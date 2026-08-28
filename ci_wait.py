import sys, time, subprocess, os

RUN_ID = sys.argv[1]
TAG = sys.argv[2]
OUT = sys.argv[3]
LOG = OUT + ".log"
os.makedirs(os.path.dirname(OUT) or ".", exist_ok=True)
os.makedirs(OUT, exist_ok=True)

def log(s):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(s + "\n")
    print(s, flush=True)

log(f"monitor start run={RUN_ID} tag={TAG} out={OUT}")

# 1) 轮询 CI 直到 conclusion 出结果
for i in range(120):
    try:
        out = subprocess.run(["gh", "run", "view", RUN_ID, "--json", "status,conclusion"],
                             capture_output=True, text=True, timeout=60).stdout
    except Exception as e:
        log(f"view err {e}")
        time.sleep(15); continue
    log(f"poll {i}: {out.strip()}")
    if '"conclusion":"success"' in out:
        log("BUILD_SUCCESS")
        break
    if '"conclusion":"failure"' in out or '"conclusion":"cancelled"' in out:
        log("BUILD_FAILED")
        sys.exit(2)
    time.sleep(20)
else:
    log("BUILD_TIMEOUT")
    sys.exit(3)

# 2) 下载 portable-x64.zip（draft release 用 gh CLI 直接取）
log("downloading release asset...")
r = subprocess.run(["gh", "release", "download", TAG, "--pattern", "*portable-x64.zip", "-D", OUT],
                  capture_output=True, text=True, timeout=300)
log(f"download rc={r.returncode}")
log(r.stdout.strip())
log(r.stderr.strip())
if r.returncode != 0:
    sys.exit(4)
log("DOWNLOAD_DONE")
