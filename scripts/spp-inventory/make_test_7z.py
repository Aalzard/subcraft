import lzma,zlib,struct,random,io
def num(n):
    if n<0x80: return bytes([n])
    for i in range(1,9):
        if n < (1<<(7*(i+1))):
            first=(0xff<<(8-i))&0xff | (n>>(8*i))
            return bytes([first])+(n&((1<<(8*i))-1)).to_bytes(i,'little')
    return b'\xff'+n.to_bytes(8,'little')
def lz(data):
    f=[{"id":lzma.FILTER_LZMA1,"dict_size":1<<16,"lc":3,"lp":0,"pb":2}]
    c=lzma.compress(data,format=lzma.FORMAT_RAW,filters=f)
    props=bytes([(2*5+0)*9+3])+struct.pack('<I',1<<16)
    return c,props
def coder(method,props):
    if method=='copy': return bytes([0x01,0x00])
    return bytes([0x23,3,1,1,len(props)])+props
def streams_info(packpos,packsize,coderb,unpack,subs=None):
    b=bytes([0x06])+num(packpos)+num(1)+bytes([0x09])+num(packsize)+bytes([0x00])
    b+=bytes([0x07,0x0b])+num(1)+bytes([0])+num(1)+coderb+bytes([0x0c])+num(unpack)+bytes([0x00])
    if subs:
        b+=bytes([0x08,0x0d])+num(len(subs))+bytes([0x09])+b''.join(num(s) for s in subs[:-1])+bytes([0x00])
    return b+bytes([0x00])
def make(files,method,encode_header):
    blob=b''.join(d for _,d in files)
    if method=='copy': packed,props=blob,b''
    else: packed,props=lz(blob)
    names=b''.join(n.encode('utf-16-le')+b'\0\0' for n,_ in files)
    hdr=bytes([0x01,0x04])+streams_info(0,len(packed),coder(method,props),len(blob),[len(d) for _,d in files])
    hdr=hdr[:-1]  # streams info ends with its own 0x00 already included; keep one terminator
    hdr+=bytes([0x00])  # end of main streams info
    hdr=bytes([0x01,0x04])+streams_info(0,len(packed),coder(method,props),len(blob),[len(d) for _,d in files])+bytes([0x05])+num(len(files))+bytes([0x11])+num(1+len(names))+bytes([0])+names+bytes([0x00])+bytes([0x00])
    body=packed
    if encode_header:
        hp,hprops=lz(hdr)
        enc=bytes([0x17])+streams_info(len(packed),len(hp),coder('lzma',hprops),len(hdr))
        body=packed+hp; hdr=enc
    start=struct.pack('<QQI',len(body),len(hdr),zlib.crc32(hdr))
    return b'7z\xbc\xaf\x27\x1c\x00\x04'+struct.pack('<I',zlib.crc32(start))+start+body+hdr
if __name__=='__main__':
    random.seed(7)
    text=b"The quick brown fox jumps over the lazy dog. "*40
    rnd=bytes(random.randrange(256) for _ in range(900))
    files=[('a/hello.txt',text),('b.bin',rnd+rnd[:300]+text[:100])]
    open('lzma.7z','wb').write(make(files,'lzma',True))
    open('copy.7z','wb').write(make(files,'copy',False))
    jpg=open('/tmp/spp/fx/embedded.jpg','rb').read()
    sbsasm=b'SBAM'+bytes(56)+jpg+bytes(range(200))
    open('embedded.7z','wb').write(make([('assemblies/content/0000/g.sbsasm',sbsasm),('assemblies/content/0000/g.xml',b'<sbsdescription/>')],'lzma',True))
