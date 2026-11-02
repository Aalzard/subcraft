import docmod,glob,collections,os,json
sbinfo=json.load(open('sbinfo.json'))
def acts(stack):
    if not stack: return
    for a in stack.get('items',[]):
        if a['_c']=='DataActionGroup': yield from acts(a.get('subStack'))
        else: yield a
def layers(items):
    for l in items:
        yield l
        if l['_c']=='DataLayerGroup': yield from layers(l['subStack']['items'])
G=collections.defaultdict(lambda: dict(uses=collections.Counter(),files=set(),variants=set(),hashes=set(),outs=set(),tweaks=collections.defaultdict(set),maps=set(),imgin=set()))
for f in sorted(glob.glob('docs/*.bin')):
    n=os.path.basename(f)[:-4]; r=docmod.parse(open(f,'rb').read())
    for m in r['materials']:
        for l in layers(m['stacks'][0]['stack']['items']):
            for ctx,key in (('content','actions'),('mask','maskActions')):
                for a in acts(l.get(key)):
                    ps=[]
                    if a['_c']=='DataActionFill':
                        ps=[('fill',s['procedural']) for s in a['sources'] if s['_c']=='DataSourceProcedural']
                    elif a['_c']=='DataActionFilterProcedural': ps=[('filter',a['procedural'])]
                    for kind,p in ps:
                        u,h=p['urlToSbsRes'].split('?version=')
                        canon=u.replace('resource://','').split('/')[-2]
                        e=G[canon]; e['uses'][(kind,ctx)]+=1; e['files'].add(n); e['variants'].add(u.split('/')[0]); e['hashes'].add(h.replace('.sbsar',''))
                        for o in p['outputs']: e['outs'].add(o['identifier'])
                        for t in p['tweaks']:
                            if not t['identifier'].startswith('$'): e['tweaks'][t['identifier']].add(round(t['value'],3) if isinstance(t.get('value'),float) else str(t.get('value')))
                        for inp in p.get('sourcesInputs',[]):
                            if inp.get('input') and inp.get('source'):
                                s=inp['source']['_c'].replace('DataSource','')
                                e['maps'].add(inp['input']['identifier']+':'+s)
rows=[]
for c,e in G.items():
    tot=sum(e['uses'].values()); h=sorted(e['hashes'])[0]
    inf=sbinfo.get(h+'.sbsar')
    sz=os.path.getsize('sbsar/%s.sbsar'%h)
    rows.append((tot,c,e,inf,sz))
rows.sort(key=lambda r:-r[0])
print(len(rows),'graphs; total uses',sum(r[0] for r in rows))
cat=collections.Counter()
for tot,c,e,inf,sz in rows:
    kinds={k for k,_ in e['uses']}
    if 'filter' in kinds: k='filter'
    elif 'basecolor' in e['outs'] or 'basecol' in e['outs'] or len(e['outs'])>2: k='material (PBR outputs)'
    elif c.startswith('mg_'): k='mask generator (mesh-map driven)'
    else: k='pattern / grunge / noise'
    cat[k]+=tot; e['cat']=k
print(dict(cat))
for tot,c,e,inf,sz in rows:
    ctx=collections.Counter()
    for (k,x),v in e['uses'].items(): ctx[x]+=v
    print('| %s | %s | %d | %d | %s | %s | %.0f KB | %s | %s |'%(c,e['cat'].split(' ')[0],tot,len(e['files']),'/'.join('%s %d'%(k,v) for k,v in ctx.items()), ', '.join(sorted(e['maps'])) or '-', sz/1024, (inf[0][3] if inf else '?'), ', '.join(list(e['tweaks'])[:7])))
