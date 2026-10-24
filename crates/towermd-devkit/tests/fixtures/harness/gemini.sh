#!/bin/sh
# Mock Gemini CLI for the runner tests: prints the same shape of
# stream-json events the documented CLI prints. Never calls a real harness.
if [ "$1" = "--version" ]; then
  echo "0.9.0"
  exit 0
fi
echo '{"type":"init","session_id":"g-1","model":"m"}'
printf '%s\n' '{"type":"message","role":"assistant","content":"先说明。\n```json\n{\"status\":\"needs_input\",","delta":true}'
printf '%s\n' '{"type":"message","role":"assistant","content":"\"summary\":\"要选库\",\"questions\":[\"用哪个库？\"]}\n```","delta":true}'
echo '{"type":"result","status":"success","stats":{"input_tokens":1,"output_tokens":2}}'
