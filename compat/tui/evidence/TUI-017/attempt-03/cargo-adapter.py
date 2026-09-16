import os
import sys
args = sys.argv[1:]
if len(args) >= 2 and args[-2] == '--jobs':
    jobs = args[-2:]
    args = args[:-2]
    if args and args[0] != 'fmt':
        pos = args.index('--') if '--' in args else len(args)
        args[pos:pos] = jobs
os.execv('/home/demfabris/.cargo/bin/cargo', ['cargo', *args])
