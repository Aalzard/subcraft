import struct,sys
d=b''
pos=0
classes=[]
def skip(n):
    global pos; pos+=n
def u32():
    global pos; v=struct.unpack_from('<I',d,pos)[0]; pos+=4; return v
def i32():
    global pos; v=struct.unpack_from('<i',d,pos)[0]; pos+=4; return v
def f32():
    global pos; v=struct.unpack_from('<f',d,pos)[0]; pos+=4; return v
def u64():
    global pos; v=struct.unpack_from('<Q',d,pos)[0]; pos+=8; return v
def u8():
    global pos; v=d[pos]; pos+=1; return v
def string():
    global pos
    n=u32(); s=d[pos:pos+n]; pos+=n; return s.decode('utf8','replace')
TRACE=0
def value(t,name,depth):
    global pos
    if t==1: return f32()
    if t==9: return i32()
    if t==11: return u32()
    if t==10: return u8()
    if t in (12,21): return u64()
    if t==16: return string()
    if t==2: return (f32(),f32())
    if t==3: return (f32(),f32(),f32())
    if t==18: return ref(depth+1)
    if t==19:
        n=u32(); return [ref(depth+1) for _ in range(n)]
    if t==6: return (i32(),i32())
    if t==15: return (u32(),u32())
    if t==5: return i32()
    if t==13: return (skip(36),struct.unpack('<9f',d[pos-36:pos]))[1]
    if t==14: return (skip(64),struct.unpack('<16f',d[pos-64:pos]))[1]
    if t==4: return (skip(16),struct.unpack('<4f',d[pos-16:pos]))[1]
    if t==20:
        return ref(depth+1)
    if t==17:
        n=u32()
        if n==0: return []
        cls=readcls()
        return [fields(cls,depth+1,0) for _ in range(n)]
    raise Exception('type %d for %s at %d'%(t,name,pos))
def ref(depth):
    global pos
    if struct.unpack_from('<I',d,pos)[0]==0:
        pos+=4; return None
    return obj(depth)
def readcls():
    cls=u32()
    if cls==0xffffffff:
        name=string(); n=u32(); props=[]
        for i in range(n):
            pn=string(); pt=u32(); props.append((pn,pt))
        classes.append((name,props)); cls=len(classes)-1
    return cls
def obj(depth=0):
    start=pos
    oid=u32(); cls=readcls()
    return fields(cls,depth,oid)
def fields(cls,depth,oid):
    start=pos
    name,props=classes[cls]
    r={'_id':oid,'_c':name}
    if TRACE: print('  '*depth+f'{start}: {name} id={oid:#x}')
    for pn,pt in props:
        r[pn]=value(pt,pn,depth)
        if TRACE>1 and not isinstance(r[pn],(dict,list)): print('  '*depth+f'  {pn}={r[pn]!r}')
    return r

def parse(data):
    global d,pos,classes
    d=data; pos=0; classes=[]
    u32();u32();u32();u32();u32()
    return obj()
