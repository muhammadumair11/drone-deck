"""Local dataset provenance, offline checks, evaluation and project scaffolding."""
import csv
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import statistics
import sys
import uuid

ROOT=Path.home()/'.local/share/drone-deck'
NAME=re.compile(r'^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$')
def emit(value): print(json.dumps(value),flush=True)
def named(value):
    if not isinstance(value,str) or not NAME.fullmatch(value): raise ValueError('Use 1–64 letters, numbers, underscores or hyphens.')
    return value
def digest(path):
    hash_=hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda:stream.read(1024*1024),b''): hash_.update(chunk)
    return hash_.hexdigest()
def save(path,value):
    path.parent.mkdir(parents=True,exist_ok=True)
    temporary=path.with_name(path.name+'.'+uuid.uuid4().hex+'.tmp')
    temporary.write_text(json.dumps(value,indent=2),encoding='utf-8');temporary.replace(path)
def recordings():
    result=[]
    for path in sorted((ROOT/'recordings').glob('*/session.json')):
        try:
            info=json.loads(path.read_text()); info['id']=path.parent.name
            if str(info.get('status','')).lower()=='finalized':result.append(info)
        except (OSError,ValueError):pass
    return result[-100:]
def dataset_path(name,version):
    named(name)
    if not re.fullmatch('[a-f0-9]{64}',str(version)):raise ValueError('Choose an immutable dataset version.')
    return ROOT/'datasets'/name/(version+'.json')
def dataset_create(request):
    name=named(request['name']);session=request['session']
    if not isinstance(session,str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,110}',session):raise ValueError('Select a valid recording session identifier.')
    recording=(ROOT/'recordings'/session).resolve()
    if recording.parent!=(ROOT/'recordings').resolve():raise ValueError('Session escapes the recording library.')
    manifest=json.loads((recording/'session.json').read_text())
    if str(manifest.get('status','')).lower()!='finalized':raise ValueError('Stop/finalize this recording before creating a dataset version.')
    files=[]
    for path in sorted((recording/'bag').rglob('*')):
        if path.is_file():
            if path.is_symlink() or not path.resolve().is_relative_to(recording):raise ValueError('Recording contains an external symbolic link.')
            emit({'progress':'Hashing recording artifact '+path.name})
            files.append(dict(path=str(path),bytes=path.stat().st_size,sha256=digest(path)))
    if not any(f['path'].endswith('.mcap') for f in files):raise ValueError('This session has no MCAP artifacts.')
    content=dict(name=name,session=session,source=manifest,artifacts=files,notes=str(request.get('notes',''))[:4000],schema=1)
    version=hashlib.sha256(json.dumps(content,sort_keys=True).encode()).hexdigest()
    content['version']=version
    destination=dataset_path(name,version)
    content['manifest']=str(destination)
    if not destination.exists():save(destination,content)
    return dict(message='Immutable dataset version created; source recordings were referenced, not copied or modified.',dataset=content)
def dataset_verify(request):
    manifest=json.loads(dataset_path(request['name'],request['version']).read_text())
    content={key:value for key,value in manifest.items() if key not in ('version','manifest')}
    if manifest.get('version')!=request['version'] or hashlib.sha256(json.dumps(content,sort_keys=True).encode()).hexdigest()!=request['version']:raise ValueError('Dataset manifest changed; restore the original immutable version.')
    missing=[]
    for file in manifest['artifacts']:
        path=Path(file['path'])
        emit({'progress':'Verifying '+path.name})
        if not path.is_file() or path.stat().st_size!=file['bytes'] or digest(path)!=file['sha256']:missing.append(str(path))
    return dict(message='Dataset artifacts verified locally.' if not missing else 'Dataset has missing or changed source artifacts.',ready=not missing,missing=missing)
def evaluate(request):
    reference=dataset_path(request['name'],request['version'])
    dataset=json.loads(reference.read_text())
    checked=dataset_verify(request)
    if not checked['ready']:raise ValueError('Restore the original dataset artifacts before evaluating this version.')
    source=Path(request['csv']).expanduser().resolve()
    if not source.is_file() or source.stat().st_size>256*1024**2:raise ValueError('Choose a predictions CSV no larger than 256 MiB.')
    task=request.get('task','classification')
    if task not in ('classification','regression'):raise ValueError('Task must be classification or regression.')
    rows=[];ids=set();latencies=[]
    with source.open(newline='',encoding='utf-8-sig') as stream:
        reader=csv.DictReader(stream)
        if not {'sample_id','target','prediction'}.issubset(reader.fieldnames or []):raise ValueError('CSV requires sample_id,target,prediction columns; latency_ms is optional.')
        for row in reader:
            if len(rows)>=1_000_000:raise ValueError('CSV exceeds one million samples.')
            key=row['sample_id']
            if not key or key in ids:raise ValueError('Sample identifiers must be nonempty and unique.')
            ids.add(key)
            if task=='regression':
                actual,predicted=float(row['target']),float(row['prediction'])
                if not math.isfinite(actual) or not math.isfinite(predicted):raise ValueError('Regression values must be finite.')
            else:actual,predicted=row['target'],row['prediction']
            rows.append((actual,predicted))
            if row.get('latency_ms'):
                latency=float(row['latency_ms'])
                if not math.isfinite(latency) or latency<0:raise ValueError('Latency must be finite and nonnegative.')
                latencies.append(latency)
    if not rows:raise ValueError('CSV has no samples.')
    if task=='classification':
        confusion={}
        for actual,predicted in rows:
            confusion.setdefault(actual,{})[predicted]=confusion.setdefault(actual,{}).get(predicted,0)+1
        metrics=dict(accuracy=sum(a==p for a,p in rows)/len(rows),confusion=confusion)
    else:
        errors=[p-a for a,p in rows];metrics=dict(mae=statistics.fmean(abs(e) for e in errors),rmse=math.sqrt(statistics.fmean(e*e for e in errors)))
    if latencies:
        ordered=sorted(latencies);metrics.update(latency_mean_ms=statistics.fmean(latencies),latency_p95_ms=ordered[max(0,math.ceil(.95*len(ordered))-1)])
    report=dict(id=uuid.uuid4().hex,dataset=dataset['name'],version=dataset['version'],task=task,samples=len(rows),metrics=metrics,
                predictions=str(source),predictions_sha256=digest(source),created=datetime.now(timezone.utc).isoformat(),
                limitation='Scores evaluate supplied predictions/targets. Sensor-frame labeling and model inference are provided by your project adapter.')
    run_manifest=source.with_name('run.json')
    if run_manifest.is_file():
        run=json.loads(run_manifest.read_text())
        if run.get('predictions_sha256')==report['predictions_sha256']:
            report['inference']={key:run.get(key) for key in ('model','model_sha256','inputs','inputs_sha256','runtime','numpy','python','provider','latency_note')}
    save(ROOT/'evaluations'/(report['id']+'.json'),report)
    return dict(message='Evaluation saved with dataset version and predictions hash.',report=report)
def library():
    datasets=[];reports=[];projects=[]
    for folder,pattern,target in [('datasets','*/*.json',datasets),('evaluations','*.json',reports),('projects','*/project.json',projects)]:
        for path in sorted((ROOT/folder).glob(pattern))[-100:]:
            try:
                value=json.loads(path.read_text());value['manifest']=str(path);target.append(value)
            except (OSError,ValueError):pass
    return dict(datasets=datasets,reports=reports,projects=projects,sessions=recordings(),message='Local dataset, evaluation and project library loaded.')
def project_create(request):
    name=named(request['name']);kind=request.get('kind','onnx');path=ROOT/'projects'/name
    if path.exists():raise ValueError('That project already exists. Choose a new name; existing files are preserved.')
    if kind not in ('onnx','ros2','mavsdk','training'):raise ValueError('Choose ONNX, ROS 2, MAVSDK or CPU training.')
    path.mkdir(parents=True)
    try:
        model=str(request.get('model',''));dataset=str(request.get('dataset',''))
        manifest=dict(name=name,kind=kind,path=str(path),model=model,dataset=dataset,distro=os.environ.get('WSL_DISTRO_NAME',''),created=datetime.now(timezone.utc).isoformat(),schema=1)
        template=Path(__file__).with_name('templates')/(kind+'.py')
        (path/'main.py').write_text(template.read_text(),encoding='utf-8')
        (path/'README.md').write_text('# '+name+'\n\nGenerated '+kind+' project.\n\nRun in the selected WSL distribution. '+
            'No dependency installation or flight commands were performed by project creation.\n'+
            ({'onnx':'Use the managed ONNX environment with an ONNX model and NPZ named tensors. Optional integer targets export classification predictions.csv; latency is amortized batch time.\n',
              'training':'Use the CPU training environment. Prepare an NPZ file with float32 NxD features and N integer class targets (minimum four samples). The seed-42 80/20 split is a baseline, not a flight-safety validation. Artifacts include model.onnx, validation.npz, weights.pt and training.json. Run ONNX inference on validation.npz, then evaluate predictions.csv against the selected dataset version. Sensor extraction/label alignment is your project adapter responsibility.\n'}.get(kind,'Use the matching environment configured in Setup. This starter monitors data and never arms or flies.\n')),encoding='utf-8')
        save(path/'project.json',manifest)
    except Exception:
        # Preserve incomplete source for inspection; never delete user project material.
        raise
    return dict(message='Project created without installing dependencies or starting code.',project=manifest)
def dispatch(request):
    action=request['action']
    return {'data_library':library,'dataset_create':lambda:dataset_create(request),'dataset_verify':lambda:dataset_verify(request),
            'evaluate':lambda:evaluate(request),'project_create':lambda:project_create(request)}[action]()
if __name__=='__main__':
    request={}
    try:
        request=json.load(sys.stdin);emit(dict(ok=True,action=request.get('action'),data=dispatch(request)))
    except Exception as error:emit(dict(ok=False,action=request.get('action'),error=str(error)))
