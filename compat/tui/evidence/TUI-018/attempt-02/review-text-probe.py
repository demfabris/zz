import os, subprocess, tempfile, shutil, time
from pathlib import Path
for side in ('tmux','zz'):
    d=Path(tempfile.mkdtemp(prefix='zzprobe-text018-',dir='/tmp'))
    env={k:v for k,v in os.environ.items() if k not in ('TMUX','TMUX_PANE','ZZ_SOCKET','ZZ_PANE','ZZ_SESSION','ZZ_DEV_BUILD','XDG_STATE_HOME')}
    env.update(HOME=str(d),XDG_CONFIG_HOME=str(d/'config'),ZZ_TRAY='0',ZZ_LOG_DIR=str(d/'logs'),TMUX_TMPDIR='/tmp')
    (d/'config').mkdir()
    socket='/tmp/zt018-'+d.name[-8:]+'.sock'
    base=['/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux','-L','zzprobe-'+d.name[-8:],'-f','/dev/null'] if side=='tmux' else ['/home/demfabris/dev/zz-box-reds/target/debug/zz','--socket',socket,'-f','/dev/null']
    def run(args,data=b''):
        p=subprocess.run(base+args,input=data,capture_output=True,env=env,timeout=10)
        print(side,repr(args),p.returncode,repr(p.stdout),repr(p.stderr),flush=True)
        return p
    try:
        run(['new-session','-d','-s','p','-n','w','-x','80','-y','24','sleep 60'])
        if side=='tmux': run(['send-keys','-l','-t','p:w.0','--','--literal-text'])
        else: run(['send-text','--no-enter','-t','p:w.0'],b'--literal-text')
        for _ in range(40):
            p=run(['capture-pane','-p','-t','p:w.0','-S','0','-E','0'])
            if b'--literal-text' in p.stdout: break
            time.sleep(.05)
        assert p.stdout==b'--literal-text\n'
        if side=='zz': run(['agent-send','-t','p:w.0'],b'--literal-agent')
    finally:
        run(['kill-server'])
        shutil.rmtree(d)
        Path(socket).unlink(missing_ok=True)
