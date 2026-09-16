import os, subprocess, tempfile, shutil
from pathlib import Path
pin='/home/demfabris/dev/zz-c11-alias/compat/.cache/tmux-src/tmux'
zz=os.environ.get('ZZ_BIN', '/home/demfabris/dev/zz-c11-alias/target/debug/zz')
for repeat in range(2):
    for side in ('tmux','zz'):
        d=Path(tempfile.mkdtemp(prefix='zzprobe-cfg018-',dir='/tmp'))
        env={k:v for k,v in os.environ.items() if k not in ('TMUX','TMUX_PANE','ZZ_SOCKET','ZZ_PANE','ZZ_SESSION','ZZ_DEV_BUILD','XDG_STATE_HOME')}
        env.update(HOME=str(d),XDG_CONFIG_HOME=str(d/'config'),ZZ_TRAY='0',ZZ_LOG_DIR=str(d/'logs'),TMUX_TMPDIR='/tmp')
        (d/'config').mkdir()
        socket='/tmp/zc018-'+d.name[-8:]+'.sock'
        base=[pin,'-L','zzprobe-'+d.name[-8:],'-f','/dev/null'] if side=='tmux' else [zz,'--socket',socket,'-f','/dev/null']
        def run(args,data=b''):
            p=subprocess.run(base+args,input=data,capture_output=True,env=env,timeout=15)
            print(repeat,side,repr(args),p.returncode,repr(p.stdout),repr(p.stderr),flush=True)
            return p
        try:
            run(['new-session','-d','-s','p','sleep 60'])
            for label,body in [('two','source-file - ; source-file -'),('one','source-file -'),('second','set -g @nonreader 1 ; source-file -')]:
                run(['set','-s','command-alias[77]','reviewcfg='+body])
                config=d/'test.conf'
                config.write_text('reviewcfg\n')
                run(['set','-gu','@reviewcfg'])
                run(['source-file',str(config)],b'set -ag @reviewcfg once\n')
                run(['show-options','-gqv','@reviewcfg'])
        finally:
            run(['kill-server'])
            shutil.rmtree(d)
            Path(socket).unlink(missing_ok=True)
