"""Process-level staging crash/restart and missing-source checks (Linux only)."""
import argparse, subprocess, time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('binary');p.add_argument('source',type=Path);p.add_argument('root',type=Path);a=p.parse_args()
a.root.mkdir(parents=True,exist_ok=True)
child=subprocess.Popen([a.binary,str(a.source),str(a.root)],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
end=time.monotonic()+20
while time.monotonic()<end:
    files=list(a.root.glob('import-*/stage.sqlite3'))
    if files and files[0].stat().st_size>100000:break
    if child.poll() is not None:raise RuntimeError('process ended before fault injection')
    time.sleep(.005)
else:
    child.kill();child.wait();raise RuntimeError('no stage writes observed')
child.kill();child.wait()
assert list(a.root.glob('import-*')), 'crash should leave discoverable partial state'
retry=subprocess.run([a.binary,str(a.source),str(a.root)],capture_output=True,text=True)
assert retry.returncode==0,retry.stderr
assert not list(a.root.glob('import-*')), 'restart must clean partial and successful stage'
missing=subprocess.run([a.binary,str(a.source)+'.missing',str(a.root)],capture_output=True,text=True)
assert missing.returncode!=0
print('PASS: SIGKILL, startup recovery, repeat ingestion, cleanup, missing source')
