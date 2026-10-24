#!/bin/sh
# Mock Codex for the runner tests: prints the same shape of `exec --json`
# events the real CLI prints. Never calls the real harness.
#   MOCK_MODE    done | noreport
#   MOCK_ARGS    file that receives the arguments, one per line
#   MOCK_PROMPT  file that receives standard input
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.163.0"
  exit 0
fi
[ -n "$MOCK_ARGS" ] && printf '%s\n' "$@" > "$MOCK_ARGS"
if [ -n "$MOCK_PROMPT" ]; then cat > "$MOCK_PROMPT"; else cat > /dev/null; fi
echo '{"type":"thread.started","thread_id":"th-1"}'
echo '{"type":"turn.started"}'
case "${MOCK_MODE:-done}" in
done)
  echo '{"type":"item.completed","item":{"id":"i-1","type":"agent_message","text":"{\"status\":\"blocked\",\"summary\":\"等依赖\",\"checkpoints\":[],\"changed_paths\":[],\"questions\":[],\"suspected_class\":null,\"proposal\":null}"}}'
  ;;
noreport)
  echo '{"type":"item.completed","item":{"id":"i-1","type":"agent_message","text":"没做完"}}'
  ;;
esac
echo '{"type":"turn.completed","usage":{"input_tokens":9,"cached_input_tokens":4,"output_tokens":2}}'
