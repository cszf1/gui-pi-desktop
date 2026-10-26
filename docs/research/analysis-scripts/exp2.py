from load import *
import numpy as np, collections, random
S=sessions()
BASE=None
def feats(acts):
    rows=[]; last=None
    def known_fail(k,i):
        b=acts[k]
        if b.get('is_error') or b.get('stale_error') or b.get('effect')=='none': return True
        return k<=i-2 and acts[k+1]['sig']==b['sig']   # retro flag already emitted
    for i,a in enumerate(acts):
        kf=[known_fail(k,i) for k in range(i)]
        n=i; fails=sum(kf); consec=0
        for v in reversed(kf):
            if v: consec+=1
            else: break
        sigfail={acts[k]['sig'] for k in range(i) if kf[k]}
        f=dict(a['features']); 
        global BASE
        if BASE is None: BASE=list(f.keys())
        g={}
        g['prev_none']=1.0 if last is not None and last.get('effect')=='none' else 0.0
        g['prev_unverif']=1.0 if last is not None and last.get('effect')=='unverifiable' else 0.0
        g['sess_rate']=math.log((fails+1)/(n-fails+4))  # shrunk log-odds of failure
        g['consec']=min(consec,3)
        g['sig_failed']=1.0 if a['sig'] in sigfail else 0.0
        t=a['tool']
        for k in ['click','type_text','press_key','scroll','set_value','hotkey','double_click','right_click']: g['t_'+k]=1.0 if t==k else 0.0
        rows.append((f,g,a['y']))
        last=a
    return rows
data=[(i,feats(acts)) for i,(f,e,acts) in enumerate(S) if acts]
def mat(rows,use):
    X=[];Y=[]
    for f,g,y in rows:
        v=[f.get(k,0.0) for k in BASE]
        if use: v+= [g[k] for k in sorted(g)]
        X.append(v);Y.append(y)
    return np.array(X),np.array(Y)
def fit(X,Y,l2=0.05,it=600):
    w=np.zeros(X.shape[1]); 
    for _ in range(it):
        p=1/(1+np.exp(-X@w)); g=X.T@(p-Y)/len(Y)+l2*np.r_[0,w[1:]]*0+l2*w*(np.arange(len(w))>0)
        w-=1.0*g
    return w
def auc(p,y):
    o=np.argsort(p); r=np.empty(len(p)); r[o]=np.arange(1,len(p)+1)
    n1=y.sum(); n0=len(y)-n1
    return (r[y==1].sum()-n1*(n1+1)/2)/(n1*n0)
def ece(p,y,b=10):
    e=0
    for i in range(b):
        m=(p>=i/b)&(p<(i+1)/b)
        if m.sum(): e+=m.sum()/len(p)*abs(p[m].mean()-y[m].mean())
    return e
random.seed(0); idx=list(range(len(data))); random.shuffle(idx)
K=5
for use in [False,True]:
    P=[];Yall=[]
    for k in range(K):
        te={idx[j] for j in range(len(idx)) if j%K==k}
        tr=[r for i,rows in data if i not in te for r in rows]
        ts=[r for i,rows in data if i in te for r in rows]
        X,Y=mat(tr,use); w=fit(X,Y); Xt,Yt=mat(ts,use)
        P+=list(1/(1+np.exp(-Xt@w))); Yall+=list(Yt)
    P=np.array(P);Yall=np.array(Yall)
    print('history' if use else 'base', 'AUC %.3f ECE %.3f Brier %.4f n=%d'%(auc(P,Yall),ece(P,Yall),((P-Yall)**2).mean(),len(P)))
# final weights for history
X,Y=mat([r for i,rows in data for r in rows],True); w=fit(X,Y)
names=BASE+sorted(data[0][1][0][1].keys())
for n_,v in sorted(zip(names,w),key=lambda x:-abs(x[1]))[:20]: print('%-14s %.2f'%(n_,v))
# operating points with history model (CV preds)
P=[];Yall=[]
for k in range(K):
    te={idx[j] for j in range(len(idx)) if j%K==k}
    tr=[r for i,rows in data if i not in te for r in rows]; ts=[r for i,rows in data if i in te for r in rows]
    X,Y=mat(tr,True); w=fit(X,Y,it=3000); Xt,Yt=mat(ts,True); P+=list(1/(1+np.exp(-Xt@w))); Yall+=list(Yt)
P=np.array(P);Y=np.array(Yall)
print('converged AUC %.3f ECE %.3f'%(auc(P,Y),ece(P,Y)))
for th in [0.4,0.5,0.6,0.7,0.75,0.8]:
    m=P<th; print('p<%.2f flag %4.1f%% of actions, catches %4.1f%% of failures, precision %4.1f%%'%(th,100*m.mean(),100*(m&(Y==0)).sum()/(Y==0).sum(),100*(m&(Y==0)).sum()/max(m.sum(),1)))
