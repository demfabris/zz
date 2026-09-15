import os, subprocess, tempfile, shutil, sys
from pathlib import Path
root=Path('/home/demfabris/dev/zz-box-reds')
pin='/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux'
for side in sys.argv[1:] or ['tmux','zz']:
 d=Path(tempfile.mkdtemp(prefix='zzprobe018-',dir='/tmp'))
 env={k:v for k,v in os.environ.items() if k not in ['TMUX','TMUX_PANE','ZZ_SOCKET','ZZ_PANE','ZZ_SESSION','ZZ_DEV_BUILD','XDG_STATE_HOME']}
 env.update(HOME=str(d),XDG_CONFIG_HOME=str(d/'config'),ZZ_TRAY='0',ZZ_LOG_DIR=str(d/'out'),TMUX_TMPDIR='/tmp')
 (d/'config').mkdir()
 socket='/tmp/z018-'+d.name[-8:]+'.sock'
 base=[pin,'-L','zzprobe-'+d.name[-8:],'-f','/dev/null'] if side=='tmux' else [str(root/'target/debug/zz'),'--socket',socket,'-f','/dev/null']
 def run(args,input=None):
  p=subprocess.run(base+args,input=input,capture_output=True,env=env,timeout=15)
  print(side,repr(args),'exit',p.returncode,'stdout',repr(p.stdout),'stderr',repr(p.stderr),flush=True)
  return p
 try:
  run(['new-session','-d','-s','probe','sleep 300'])
  for i,(alias,body,payload) in enumerate([
   ('one','source-file -',b'set -g @zzprobe-one single\n'),
   ('two','source-file - ; source-file -',b'set -g @zzprobe-one twice\n'),
   ('loadsource','load-buffer -b probe - ; source-file -',b'a\xff\0z\n'),
   ('loadtwo','load-buffer -b probe - ; load-buffer -b second - ; display-message -p after-error',b'a\xff\0z\n'),
   ('sourceload','source-file - ; load-buffer -b probe - ; display-message -p after-error',b'set -g @zzprobe-one sourcefirst\n'),
   ('loadprint','load-buffer -b probe - ; display-message -p after',b'a\xff\0z\n'),
   ('printload','display-message -p before ; load-buffer -b probe -',b'a\xff\0z\n'),
  ]):
   run(['set-option','-s',f'command-alias[{77+i}]',f'{alias}={body}'])
   run([alias],payload)
   run(['show-options','-gqv','@zzprobe-one'])
   if 'load' in alias: run(['save-buffer','-b','probe','-'])
 finally:
  run(['kill-server'])
  shutil.rmtree(d)
  Path(socket).unlink(missing_ok=True)
