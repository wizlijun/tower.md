#!/bin/sh
# Mock Claude Code for the runner tests: prints the same shape of
# stream-json the real CLI prints. Never calls the real harness.
#   MOCK_MODE   reply | done | commit | noreport | design | external | budget | crash | hang | stubborn
#   MOCK_REPLY  the JSON object reply mode answers with
#   MOCK_COMMIT file (relative to the worktree) that commit mode changes and commits
#   MOCK_ARGS   file that receives the arguments, one per line
#   MOCK_STATE  file that marks the first attempt (external mode)
if [ "$1" = "--version" ]; then
  echo "2.1.308 (Claude Code)"
  exit 0
fi
[ -n "$MOCK_ARGS" ] && printf '%s\n' "$@" > "$MOCK_ARGS"
report='{"status":"done","summary":"改好了","checkpoints":[],"changed_paths":["src/a/x.rs"],"questions":[],"suspected_class":null,"proposal":null}'
usage='"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":1,"cache_read_input_tokens":2}'
echo '{"type":"system","subtype":"init","session_id":"s-1","tools":[]}'
case "${MOCK_MODE:-done}" in
reply)
  # A consultation: MOCK_REPLY is the JSON object it answers with; no cost reported.
  echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,$usage,\"result\":\"\",\"structured_output\":${MOCK_REPLY:-null}}"
  ;;
done)
  echo '{"type":"assistant","message":{"content":[{"type":"text","text":"在改"}]}}'
  echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"total_cost_usd\":0.25,$usage,\"result\":\"好了\",\"structured_output\":$report}"
  ;;
commit)
  target="${MOCK_COMMIT:-src/a/x.rs}"
  mkdir -p "$(dirname "$target")"
  echo "// changed by the mock harness $$" >> "$target"
  git add -- "$target" >/dev/null &&
    git -c user.name=mock -c user.email=mock@localhost -c commit.gpgsign=false -c core.hooksPath=/dev/null \
      commit -q -m "mock change" >/dev/null || exit 3
  echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"total_cost_usd\":0.25,$usage,\"result\":\"好了\",\"structured_output\":$report}"
  ;;
noreport)
  echo '{"type":"assistant","message":{"content":[{"type":"text","text":"做了一半"}]}}'
  echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"total_cost_usd\":0.1,$usage,\"result\":\"做了一半，没写报告\"}"
  ;;
design)
  design='{"status":"failed","summary":"接口定义互相矛盾","checkpoints":[],"changed_paths":[],"questions":[],"suspected_class":"design","proposal":{"kind":"interface","reason":"两个模块对同一字段定义不同"}}'
  echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"total_cost_usd\":0.2,$usage,\"result\":\"\",\"structured_output\":$design}"
  ;;
external)
  if [ ! -e "$MOCK_STATE" ]; then
    : > "$MOCK_STATE"
    echo '{"type":"system","subtype":"api_retry","attempt":1,"max_retries":10,"retry_delay_ms":10,"error_status":529,"error":"overloaded"}'
    echo '{"type":"result","subtype":"error_during_execution","is_error":true,"total_cost_usd":0.05,"errors":["API Error: 529 overloaded"]}'
    exit 1
  fi
  echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"total_cost_usd\":0.25,$usage,\"result\":\"好了\",\"structured_output\":$report}"
  ;;
budget)
  echo '{"type":"result","subtype":"error_max_budget_usd","is_error":true,"total_cost_usd":1.2,"errors":["Reached maximum budget"]}'
  exit 1
  ;;
crash)
  kill -SEGV $$
  ;;
hang)
  trap 'echo "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"total_cost_usd\":0.3,\"errors\":[\"interrupted\"]}"; exit 130' INT
  while :; do sleep 1; done
  ;;
stubborn)
  trap '' INT
  while :; do sleep 1; done
  ;;
esac
