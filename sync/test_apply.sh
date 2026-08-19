#!/bin/zsh
# test_apply.sh — 验证 pull --apply 写回路径（合成库，不动真实库）
# 注意：DEMO 为本地测试占位 token，请勿用于任何真实账号/生产环境。
set -e
NODE=/Users/xiamuguizhi/.workbuddy/binaries/node/versions/22.22.2/bin/node
DIR=/Users/xiamuguizhi/WorkBuddy/2026-08-18-15-00-54/workbuddy-account-hub/sync
TEST=/tmp/sync-test
REAL=/Users/xiamuguizhi/.workbuddy
UIDX=de58fc75-61f2-4429-a4af-11ffcc5fc6fa
PORT=8791

rm -rf "$TEST/applydb" && mkdir -p "$TEST/applydb/memory"
cp "$REAL/workbuddy.db" "$TEST/applydb/workbuddy.db"
cp "$REAL/workbuddy.db-wal" "$TEST/applydb/workbuddy.db-wal" 2>/dev/null || true

echo "=== 1) 启动服务器(端口$PORT) ==="
$NODE "$DIR/sync-server.js" --port $PORT --data "$TEST/server-b" > "$TEST/serverb.log" 2>&1 &
SRV=$!
trap "kill $SRV 2>/dev/null || true" EXIT
sleep 1.5
curl -s -X POST http://127.0.0.1:$PORT/api/register -H 'Content-Type: application/json' -d '{"accountId":"demo2","token":"DEMO"}' >/dev/null; echo "注册完成"

echo "=== 2) 选一个真实 session，构造 synthetic 新标题并 push ==="
SID=$(python3 -c "import sqlite3;c=sqlite3.connect('$TEST/applydb/workbuddy.db');r=c.execute(\"SELECT id,title,updated_at FROM sessions WHERE user_id='$UIDX' LIMIT 1\").fetchone();open('$TEST/sid.txt','w').write(r[0]);print(r[0])")
echo "选 session: $SID"
python3 - "$TEST/applydb/workbuddy.db" "$SID" "$PORT" <<'PY'
import sqlite3,sys,json,base64,urllib.request
db,sid,port=sys.argv[1],sys.argv[2],int(sys.argv[3])
c=sqlite3.connect(db);c.row_factory=sqlite3.Row
row=dict(c.execute("SELECT * FROM sessions WHERE id=?",(sid,)).fetchone());c.close()
row['title']='[合成apply测试]新标题'
ua=row['updated_at']+86400000
rec={'id':'session:'+sid,'type':'session','owner_uid':'de58fc75-61f2-4429-a4af-11ffcc5fc6fa','data':row,'version':1,'updated_at':ua,'deleted':False}
body=json.dumps({'records':[rec]}).encode()
auth='Basic '+base64.b64encode(b'demo2:DEMO').decode()
req=urllib.request.Request(f'http://127.0.0.1:{port}/api/push',data=body,headers={'Content-Type':'application/json','Authorization':auth},method='POST')
print("push 响应:", urllib.request.urlopen(req).read().decode())
PY

echo "=== 3) applydb 本地该 session 当前标题(应为旧) ==="
python3 -c "import sqlite3;c=sqlite3.connect('$TEST/applydb/workbuddy.db');print('  apply 前:', c.execute(\"SELECT title FROM sessions WHERE id='$SID'\").fetchone()[0])"

echo "=== 4) pull --apply --force (目标=合成库) ==="
WORKBUDDY_HOME="$TEST/applydb" $NODE --experimental-sqlite "$DIR/sync-client.js" pull --account demo2 --token DEMO --uid "$UIDX" --db "$TEST/applydb/workbuddy.db" --apply --force --server "http://127.0.0.1:$PORT"

echo "=== 5) 验证 applydb 该 session 标题已更新 ==="
python3 -c "import sqlite3;c=sqlite3.connect('$TEST/applydb/workbuddy.db');print('  apply 后:', c.execute(\"SELECT title FROM sessions WHERE id='$SID'\").fetchone()[0])"
echo "测试 B(apply 写回)完成"
