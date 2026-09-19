import subprocess
for text in ['Bad file descriptor: -', 'source-file from standard input is not supported', 'command load-buffer: too many arguments (need at most 1)', 'invalid octal escape', '__zz-stdin-payload', 'source-file', 'load-buffer', 'save-buffer', 'command-alias', 'agent-send', 'send-text']:
    print(f'rg -n -F -- {text!r} compat/scenarios', flush=True)
    result = subprocess.run(['rg', '-n', '-F', '--', text, 'compat/scenarios'])
    print(f'exit={result.returncode}', flush=True)
    if result.returncode > 1:
        raise SystemExit(result.returncode)
