import py7zr,glob,collections,os,re,io,struct,json
import xml.etree.ElementTree as ET
ext=collections.Counter(); builds=collections.Counter(); ver=collections.Counter(); sizes=[]
inputs_types=collections.Counter(); keywords=collections.Counter(); nout=collections.Counter(); outfmt=collections.Counter()
info={}
os.makedirs('x/all',exist_ok=True)
for f in sorted(glob.glob('sbsar/*.sbsar')):
    z=py7zr.SevenZipFile(f)
    for i in z.list():
        if not i.is_directory: ext[os.path.splitext(i.filename)[1]]+=1
    z=py7zr.SevenZipFile(f)
    import shutil
    tmp='x/tmp'; shutil.rmtree(tmp,ignore_errors=True); z.extractall(tmp)
    data={}
    for root,_,fs in os.walk(tmp):
        for fn in fs: data[fn]=open(os.path.join(root,fn),'rb').read()
    for n,b in data.items():
        if n.endswith('.sbsasm'):
            builds[struct.unpack_from('<I',b,8)[0]]+=1; ver[(struct.unpack_from('<I',b,4)[0])]+=1; sizes.append(len(b))
            open('x/all/'+os.path.basename(f)[:8]+'.sbsasm','wb').write(b)
        if n.endswith('.xml'):
            r=ET.fromstring(re.sub(rb'<!DOCTYPE[^>]*>',b'',b))
            gs=list(r.iter('graph'))
            info[os.path.basename(f)]=[(g.get('label'),g.get('pkgurl'),g.get('keywords'),len(list(g.iter('input'))),len(list(g.iter('output')))) for g in gs]
            for g in gs:
                for k in (g.get('keywords') or '').split(','): keywords[k]+=1
                nout[len(list(g.iter('output')))]+=1
                for o in g.iter('output'): outfmt[o.get('format')]+=1
                for i in g.iter('input'): inputs_types[i.get('type')]+=1
print('entries by ext',dict(ext)); print('versions',dict(ver),'cooker builds',dict(builds))
print('asm size min/med/max',min(sizes),sorted(sizes)[len(sizes)//2],max(sizes))
print('input types',dict(inputs_types)); print('outputs/graph',dict(nout),'formats',dict(outfmt)); print(keywords.most_common(25))
json.dump(info,open('sbinfo.json','w'),indent=0)
multi=[k for k,v in info.items() if len(v)>1]; print('multi-graph archives',len(multi))
