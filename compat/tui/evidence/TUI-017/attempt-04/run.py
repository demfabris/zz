import datetime
import json
import os
import re
from pathlib import Path
import subprocess
import sys

folder = Path(__file__).resolve().parent
label, *command = sys.argv[1:]
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
with (folder / (label + '.txt')).open('w') as output:
    output.write('command: ' + json.dumps(command) + '\n')
    output.flush()
    env = {key: value for key, value in os.environ.items() if not re.search(r'(TOKEN|PASSWORD|SECRET|API_KEY|CREDENTIAL)', key)}
    result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT, env=env)
    output.write(f'\nexit_status: {result.returncode}\n')
entry = {'label': label, 'command': command, 'start': start, 'end': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'exit_status': result.returncode}
with (folder / 'runs.jsonl').open('a') as output:
    output.write(json.dumps(entry) + '\n')
print(json.dumps(entry))
sys.exit(result.returncode)
