import os, subprocess, tempfile, shutil
from pathlib import Path
for side in ('tmux','zz'):
    d=Path(tempfile.mkdtemp(prefix='zzprobe-order018-',dir='/tmp'))
    env={k:v for k,v in os.environ.items() if k not in ('TMUX','TMUX_PANE','ZZ_SOCKET','ZZ_PANE','ZZ_SESSION','ZZ_DEV_BUILD','XDG_STATE_HOME')}
    env.update(HOME=str(d),XDG_CONFIG_HOME=str(d/'config'),ZZ_TRAY='0',ZZ_LOG_DIR=str(d/'logs'),TMUX_TMPDIR='/tmp')
    (d/'config').mkdir()
    socket='/tmp/zo018-'+d.name[-8:]+'.sock'
    base=['/home/demfabris/dev/zz-c11-alias/compat/.cache/tmux-src/tmux','-L','zzprobe-'+d.name[-8:],'-f','/dev/null'] if side=='tmux' else [os.environ.get('ZZ_BIN', '/home/demfabris/dev/zz-c11-alias/target/debug/zz'),'--socket',socket,'-f','/dev/null']
    def run(args,data=b''):
        try:
            p=subprocess.run(base+args,input=data,capture_output=True,env=env,timeout=10)
            print(side,repr(args),p.returncode,repr(p.stdout),repr(p.stderr),flush=True)
        except subprocess.TimeoutExpired as e:
            print(side,repr(args),'TIMEOUT',repr(e.stdout),repr(e.stderr),flush=True)
    try:
        run(['new-session','-d','-s','p','-n','w','sleep 180'])
        cases=[
            ('missing-source','display-message -p before ; source-file /tmp/zz018-no-such-config ; display-message -p after ; set -g @after yes',b''),
            ('source-parse','source-file - ; display-message -p after ; set -g @after yes',b'no-such-command\n'),
            ('source-option','display-message -p before ; source-file -Z ; display-message -p after ; set -g @after yes',b''),
        ]
        for label,body,data in cases:
            run(['set','-gu','@after'])
            print('CASE',label,flush=True)
            run(['set','-s','command-alias[77]','revieworder='+body])
            run(['revieworder'],data)
            run(['show-options','-gqv','@order'])
            run(['show-options','-gqv','@after'])
            run(['list-panes','-t','p:w','-F','#{pane_index} #{pane_dead}'])
    finally:
        run(['kill-server'])
        shutil.rmtree(d)
        Path(socket).unlink(missing_ok=True)
