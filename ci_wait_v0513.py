import subprocess, time, os, json

REPO = "xmgzxmgz/workbuddy-account-hub"
RUN_ID = "33138914387"
TAG = "v0.5.13"
OUT = ".ci_staging/v0.5.13"
LOG = ".ci_staging/v0.5.13.log"

def gh(*args):
    return subprocess.run(["gh"] + list(args), capture_output=True, text=True)

os.makedirs(OUT, exist_ok=True)
log = open(LOG, "w")
def say(s):
    print(s); log.write(s + "\n"); log.flush()

say(f"[{time.strftime('%H:%M:%S')}] monitoring run {RUN_ID} for {TAG}")
deadline = time.time() + 18 * 60
result = "TIMEOUT"
while time.time() < deadline:
    r = gh("run", "view", RUN_ID, "--json", "status,conclusion")
    if r.returncode == 0:
        d = json.loads(r.stdout)
        st, con = d.get("status"), d.get("conclusion")
        say(f"[{time.strftime('%H:%M:%S')}] status={st} conclusion={con}")
        if st == "completed":
            if con == "success":
                say("BUILD_SUCCESS")
                dl = gh("release", "download", TAG, "--pattern", "*portable-x64.zip", "-D", OUT)
                say(f"download rc={dl.returncode} {dl.stderr[-400:]}")
                if dl.returncode == 0:
                    say(f"DOWNLOAD_DONE {os.listdir(OUT)}")
                    result = "DONE"
                else:
                    say("DOWNLOAD_FAIL")
                    result = "DLFAIL"
                break
            else:
                say(f"BUILD_FAILED {con}")
                result = "FAILED"
                break
    else:
        say(f"view err {r.stderr[-300:]}")
    time.sleep(30)
say(f"RESULT={result}")
log.close()
