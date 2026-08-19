#!/bin/zsh
# test_sync.sh — 同步核心逻辑端到端测试（dry-run，不碰真实库）
# 注意：DEMO 为本地测试占位 token，请勿用于任何真实账号/生产环境。
set -e
NODE=/Users/xiamuguizhi/.workbuddy/binaries/node/versions/22.22.2/bin/node
DIR=/Users/xiamuguizhi/WorkBuddy/2026-08-18-15-00-54/workbuddy-account-hub/sync
TEST=/tmp/sync-test
REAL_DB=/Users/xiamuguizhi/.workbuddy/workbuddy.db
UIDX=de58fc75-61f2-4429-a4af-11ffcc5fc6fa

rm -rf "$TEST" && mkdir -p "$TEST"

echo "=== 1) 启动同步服务器(端口8787) ==="
$NODE "$DIR/sync-server.js" --port 8787 --data "$TEST/server-data" > "$TEST/server.log" 2>&1 &
SRV=$!
sleep 1.5

echo "=== 2) 注册账号 demo ==="
curl -s -X POST http://localhost:8787/api/register -H 'Content-Type: application/json' -d '{"accountId":"demo","token":"DEMO"}'; echo

echo "=== 3) 设备A(真实库) push ==="
$NODE --experimental-sqlite "$DIR/sync-client.js" push --account demo --token DEMO --uid "$UIDX" --server http://127.0.0.1:8787 2>/dev/null

echo "=== 4) 服务器端状态 ==="
curl -s http://localhost:8787/api/status -H "Authorization: Basic $(echo -n 'demo:DEMO' | base64)"; echo

echo "=== 5) 设备B: 复制 db 并模拟改动(改1条标题+软删1条) ==="
cp "$REAL_DB" "$TEST/dbB.db"
python3 - "$TEST/dbB.db" <<'PY'
import sqlite3, sys
db=sys.argv[1]
c=sqlite3.connect(db); cur=c.cursor()
cur.execute("SELECT id,title,updated_at FROM sessions WHERE user_id='__UID__' LIMIT 2".replace('__UID__', sys.argv[2] if False else 'de58fc75-61f2-4429-a4af-11ffcc5fc6fa'))
rows=cur.fetchall()
id1,title1,up1=rows[0]; id2,title2,up2=rows[1]
cur.execute("UPDATE sessions SET title=?, updated_at=? WHERE id=?", ("[设备B修改]"+str(title1), up1+86400000, id1))
cur.execute("UPDATE sessions SET deleted_at=? WHERE id=?", (up2+86400000, id2))
c.commit(); c.close()
print("设备B 修改:", id1[:8], " 软删:", id2[:8])
PY

echo "=== 6) 设备B push ==="
$NODE --experimental-sqlite "$DIR/sync-client.js" push --account demo --token DEMO --uid "$UIDX" --db "$TEST/dbB.db" --server http://127.0.0.1:8787 2>/dev/null

echo "=== 7) 设备A pull (dry-run, 应看到 1 update + 1 delete) ==="
$NODE --experimental-sqlite "$DIR/sync-client.js" pull --account demo --token DEMO --uid "$UIDX" --server http://127.0.0.1:8787 2>/dev/null

echo "=== 8) 收尾: 关闭服务器 ==="
kill $SRV 2>/dev/null || true
echo "测试 A 完成。服务器日志: $TEST/server.log"
