import os, pathlib, subprocess, time, shlex, shutil, sys
root=pathlib.Path('/tmp/zz-capture-fix/probes'); root.mkdir(exist_ok=True)
D=pathlib.Path('/tmp/zz-capture-fix/home'); (D/'config').mkdir(parents=True,exist_ok=True)
env={k:v for k,v in os.environ.items() if k not in ['TMUX','TMUX_PANE','ZZ_SOCKET','ZZ_PANE','ZZ_SESSION']};env.update(HOME=str(D),XDG_CONFIG_HOME=str(D/'config'),ZZ_TRAY='0')
scenes={'colourwrap': '\x1b[31m'+'A'*170+'\x1b[0m\r\nNEXT', 'named':'\x1b[31mRED\x1b[0m\r\nNEXT','indexed':'\x1b[38;5;196mRED\x1b[0m\r\nNEXT','low-indexed':'\x1b[38;5;1mRED\x1b[0m\r\nNEXT','rgb':'\x1b[38;2;1;2;3mRGB\x1b[0m\r\nNEXT','bold':'\x1b[1mBOLD\x1b[0m\r\nNEXT','underline':'\x1b[4mUNDER\x1b[0m\r\nNEXT','charset':'\x1b(0qqq\x1b(B\r\nNEXT','history':''.join(f'H{i:02}\r\n' for i in range(35))+'NEXT','wrapped':'W'*170+'\r\nNEXT'}
scenes.update({'named-background':'\x1b[32;44mCOLOUR\x1b[0m\r\nNEXT','bright':'\x1b[91;104mBRIGHT\x1b[0m\r\nNEXT','attribute-reset':'\x1b[1;4;31mONE\x1b[22mTWO\x1b[0m\r\nNEXT'})
scenes.update({'erased-background':'\x1b[41m\x1b[2K\x1b[0m\r\nNEXT','erased-rgb-background':'\x1b[48;2;4;5;6m\x1b[2K\x1b[0m\r\nNEXT'})
for side in sys.argv[1:]:
 cmd=[os.environ.get('ZZ_COMPAT_TMUX','/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux'),'-L','zzprobe-capfix-'+str(os.getpid()),'-f','/dev/null'] if side=='tmux' else [os.environ.get('ZZ_BIN',str(pathlib.Path.cwd()/'target/debug/zz')),'--socket','/tmp/zzcf'+str(os.getpid())+'.sock']
 def run(*args): return subprocess.run(cmd+list(args),env=env,capture_output=True)
 try:
  for name,payload in scenes.items():
   command='printf %s '+shlex.quote(payload)+'; exec sleep 600'
   r=run('new-session','-d','-s',name,'-x','80','-y','24',command)
   assert r.returncode==0, r.stderr
   run('set-option','-g','lock-command','true')
   for i in range(100):
    r=run('capture-pane','-p','-t','='+name+':')
    if b'NEXT' in r.stdout:break
    time.sleep(.05)
   (root/(name+'.'+side+'.geometry')).write_bytes(run('display-message','-p','-t','='+name+':','#{pane_width}x#{pane_height}').stdout)
   flags=['-L','-e','-J','-S','0','-E','3'] if name=='colourwrap' else ['-L','-S','-3','-E','0'] if name=='history' else ['-L','-J','-S','0','-E','3'] if name=='wrapped' else ['-C','-e','-S','0','-E','0']
   r=run('capture-pane','-p','-t','='+name+':',*flags)
   (root/(name+'.'+side+'.out')).write_bytes(r.stdout)
   (root/(name+'.'+side+'.err')).write_bytes(r.stderr)
   print(side,name,r.returncode,repr(r.stdout))
   run('kill-session','-t','='+name+':')
 finally:run('kill-server')

shutil.rmtree(D)
