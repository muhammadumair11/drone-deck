"""CPU classifier baseline for explicitly prepared features/targets; exports ONNX."""
import argparse
import hashlib
import json
from pathlib import Path
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--inputs',required=True);parser.add_argument('--output',default='artifacts');parser.add_argument('--epochs',type=int,default=5)
    args=parser.parse_args()
    if not 1<=args.epochs<=500:raise ValueError('Epochs must be 1–500.')
    import numpy as np
    import torch
    torch.manual_seed(42);torch.set_num_threads(2)
    source=Path(args.inputs).resolve();output=Path(args.output);output.mkdir(parents=True,exist_ok=True)
    with np.load(source,allow_pickle=False) as data:
        features=np.asarray(data['features'],dtype=np.float32);targets=np.asarray(data['targets'])
    if features.ndim!=2 or features.shape[1]==0 or targets.shape!=(len(features),) or len(features)<4 or not np.issubdtype(targets.dtype,np.integer) or not np.isfinite(features).all() or targets.min()<0 or targets.max()>9999:raise ValueError('Provide finite NxD features and N nonnegative integer class targets, with at least four samples.')
    targets=targets.astype(np.int64)
    order=np.random.default_rng(42).permutation(len(features));split=max(1,int(.8*len(order)));train,test=order[:split],order[split:]
    x=torch.from_numpy(features);y=torch.from_numpy(targets)
    model=torch.nn.Sequential(torch.nn.Linear(features.shape[1],64),torch.nn.ReLU(),torch.nn.Linear(64,int(targets.max())+1))
    optimizer=torch.optim.Adam(model.parameters(),lr=.001);loss_function=torch.nn.CrossEntropyLoss();history=[]
    for epoch in range(args.epochs):
        model.train();losses=[]
        for start in range(0,len(train),64):
            batch=train[start:start+64];optimizer.zero_grad();loss=loss_function(model(x[batch]),y[batch]);loss.backward();optimizer.step();losses.append(float(loss.detach()))
        history.append(sum(losses)/len(losses));print(json.dumps(dict(epoch=epoch+1,loss=history[-1])),flush=True)
    model.eval()
    with torch.no_grad():accuracy=float((model(x[test]).argmax(1)==y[test]).float().mean())
    torch.save(model.state_dict(),output/'weights.pt')
    torch.onnx.export(model,x[test[:1]],str(output/'model.onnx'),input_names=['features'],output_names=['logits'],dynamic_axes={'features':{0:'batch'},'logits':{0:'batch'}},opset_version=17,dynamo=False)
    np.savez(output/'validation.npz',features=features[test],targets=targets[test])
    digest=hashlib.sha256()
    with source.open('rb') as stream:
        for block in iter(lambda:stream.read(1024*1024),b''):digest.update(block)
    (output/'training.json').write_text(json.dumps(dict(seed=42,epochs=args.epochs,input_sha256=digest.hexdigest(),torch=torch.__version__,train_samples=len(train),validation_samples=len(test),validation_accuracy=accuracy,loss=history,preprocessing='User-supplied numeric features; recording-to-feature alignment is defined by the project adapter.'),indent=2))
    print('CPU training and ONNX export complete:',output)
if __name__=='__main__':main()
