import os, subprocess, tempfile, shutil, json
from pathlib import Path
root=Path('/home/demfabris/dev/zz-box-reds')
pin='/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux'
results={}
for side in ['tmux','zz']:
    d=Path(tempfile.mkdtemp(prefix='zzprobe-review018-',dir='/tmp'))
    env={k:v for k,v in os.environ.items() if k not in ['TMUX','TMUX_PANE','ZZ_SOCKET','ZZ_PANE','ZZ_SESSION','ZZ_DEV_BUILD','XDG_STATE_HOME']}
    env.update(HOME=str(d),XDG_CONFIG_HOME=str(d/'config'),ZZ_TRAY='0',ZZ_LOG_DIR=str(d/'out'),TMUX_TMPDIR='/tmp')
    (d/'config').mkdir()
    socket='/tmp/zr018-'+d.name[-8:]+'.sock'
    base=[pin,'-L','zzprobe-'+d.name[-8:],'-f','/dev/null'] if side=='tmux' else [str(root/'target/debug/zz'),'--socket',socket,'-f','/dev/null']
    records={}
    def run(label,args,payload=b''):
        p=subprocess.run(base+args,input=payload,capture_output=True,env=env,timeout=20)
        result=(p.returncode,p.stdout.hex(),p.stderr.hex())
        print(side,label,repr(args),'exit',p.returncode,'stdout',repr(p.stdout),'stderr',repr(p.stderr),flush=True)
        if label: records[label]=result
        return p
    def alias(body):
        run('', ['set','-s','command-alias[77]','reviewstream='+body])
    try:
        run('setup',['new-session','-d','-s','probe','-n','win','-x','80','-y','24','sleep 300'])
        alias('source-file - ; source-file -')
        run('two-source',['reviewstream'],b'set -ag @review once\n')
        run('two-source-value',['show-options','-gqv','@review'])
        alias('load-buffer -b binary - ; source-file -')
        run('binary-first',['reviewstream'],b'a\xff\0z\n')
        p=run('binary-readback',['save-buffer','-b','binary','-'])
        print(side,'od -c',subprocess.check_output(['od','-c'],input=p.stdout).decode(),flush=True)
        alias('source-file -')
        run('single-alias',['reviewstream'],b'set -g @review single\n')
        run('single-alias-value',['show-options','-gqv','@review'])
        run('direct-source',['source-file','-'],b'set -g @review direct\n')
        run('direct-source-value',['show-options','-gqv','@review'])
        alias('set -g @x 1 ; source-file -')
        run('second-reader',['reviewstream'],b'set -g @review second\n')
        run('second-reader-value',['show-options','-gqv','@review'])
        run('nonreader-value',['show-options','-gqv','@x'])
        run('', ['split-window','-d','-t','probe:win.0',''])
        alias('set -g @x 2 ; display-message -I -t probe:win.1')
        run('pane-input-group',['reviewstream'],b'pane input\r\n')
        run('pane-input-readback',['capture-pane','-p','-t','probe:win.1','-S','0','-E','1'])
        alias('source-file -t probe:win.0 - ; source-file -t probe:win.1 -')
        run('source-targets',['reviewstream'],b'set -w @target yes\n')
        run('source-target-value',['show-options','-wqv','-t','probe:win','@target'])
        alias('set -g @x 3 ; split-window -d -I -c /tmp -t probe:win.0')
        run('split-target-cwd',['reviewstream'],b'split input\r\n')
        run('split-target-readback',['capture-pane','-p','-t','probe:win.1','-S','0','-E','1'])
        alias('source-file - ; source-file -')
        config=d/'invoke.conf'
        config.write_text('reviewstream\n')
        run('config-alias',['source-file',str(config)],b'set -g @review from-config\n')
        run('config-alias-value',['show-options','-gqv','@review'])
        run('direct-binary',['load-buffer','-b','binary','-'],b'd\xfe\0e\n')
        run('direct-binary-value',['save-buffer','-b','binary','-'])
        run('direct-send-keys',['send-keys','-l','-t','probe:win.0','--','--literal'])
        if side=='zz':
            run('direct-send-text',['send-text','-t','probe:win.0','--no-enter'],b'--literal')
        results[side]=records
    finally:
        run('', ['kill-server'])
        shutil.rmtree(d)
        Path(socket).unlink(missing_ok=True)
for label in results['tmux']:
    print('COMPARE',label,'IDENTICAL' if results['tmux'][label]==results['zz'].get(label) else 'DIFFERENT',flush=True)
