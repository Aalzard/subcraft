import docmod,glob,collections,os,json,sys
def walk(o,path=()):
    if isinstance(o,dict):
        yield o,path
        for k,v in o.items():
            if k.startswith('_'): continue
            yield from walk(v,path+(o['_c'],k))
    elif isinstance(o,list):
        for x in o: yield from walk(x,path)
files=sorted(glob.glob('docs/*.bin'))
classcount=collections.defaultdict(collections.Counter)
for f in files:
    r=docmod.parse(open(f,'rb').read())
    n=os.path.basename(f)[:-4]
    for o,p in walk(r):
        classcount[o['_c']][n]+=1
allc=sorted(classcount, key=lambda c:-sum(classcount[c].values()))
print('%-34s %7s %s'%('class','total','files'))
for c in allc: print('%-34s %7d %d'%(c,sum(classcount[c].values()),len(classcount[c])))
