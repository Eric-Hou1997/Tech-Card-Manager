import struct, sys
from pathlib import Path

ico_path=Path(sys.argv[1]); out_path=Path(sys.argv[2])
data=ico_path.read_bytes()
reserved,typ,count=struct.unpack_from('<HHH',data,0)
assert reserved==0 and typ==1 and count>0
icons=[]
for i in range(count):
    off=6+i*16
    w,h,cc,res,planes,bpp,size,img_off=struct.unpack_from('<BBBBHHII',data,off)
    icons.append(dict(w=w,h=h,cc=cc,res=res,planes=planes,bpp=bpp,size=size,data=data[img_off:img_off+size]))

def align4(n): return (n+3)&~3
blob=bytearray()
def alloc(n, align=4):
    while len(blob)%align: blob.append(0)
    off=len(blob); blob.extend(b'\0'*n); return off
def pack_at(off,fmt,*vals): struct.pack_into(fmt,blob,off,*vals)

def dir_header(num_ids):
    off=alloc(16)
    pack_at(off,'<IIHHHH',0,0,0,0,0,num_ids)
    return off

def dir_entry(off, idx, idv, target, subdir):
    val=target | (0x80000000 if subdir else 0)
    pack_at(off+16+idx*8,'<II',idv,val)

# Lay out all directories first.
root=dir_header(2); root_entries_extra=alloc(16)  # header included no entries; add 2 entries
assert root_entries_extra==root+16
icon_type=dir_header(count); alloc(count*8)
group_type=dir_header(1); alloc(8)
icon_lang_dirs=[]
for _ in icons:
    d=dir_header(1); alloc(8); icon_lang_dirs.append(d)
group_lang=dir_header(1); alloc(8)

# Data entries.
icon_data_entries=[alloc(16) for _ in icons]
group_data_entry=alloc(16)

# Fill directory entries.
dir_entry(root,0,3,icon_type,True)
dir_entry(root,1,14,group_type,True)
for i,d in enumerate(icon_lang_dirs):
    dir_entry(icon_type,i,i+2,d,True)
    dir_entry(d,0,0x409,icon_data_entries[i],False)
dir_entry(group_type,0,1,group_lang,True)
dir_entry(group_lang,0,0x409,group_data_entry,False)

# Resource payloads.
resource_offsets=[]
for icon in icons:
    while len(blob)%4: blob.append(0)
    o=len(blob); blob.extend(icon['data']); resource_offsets.append(o)
while len(blob)%4: blob.append(0)
group_offset=len(blob)
group=bytearray(struct.pack('<HHH',0,1,count))
for i,icon in enumerate(icons):
    group.extend(struct.pack('<BBBBHHIH', icon['w'],icon['h'],icon['cc'],icon['res'],icon['planes'],icon['bpp'],icon['size'],i+2))
blob.extend(group)
while len(blob)%4: blob.append(0)

# Fill data entries and remember relocation offsets.
relocs=[]
for de,o,icon in zip(icon_data_entries,resource_offsets,icons):
    pack_at(de,'<IIII',o,icon['size'],0,0); relocs.append(de)
pack_at(group_data_entry,'<IIII',group_offset,len(group),0,0); relocs.append(group_data_entry)

raw_size=len(blob)
raw_ptr=20+40
reloc_ptr=raw_ptr+raw_size
sym_ptr=reloc_ptr+len(relocs)*10
# COFF header
coff=bytearray()
coff.extend(struct.pack('<HHIIIHH',0x8664,1,0,sym_ptr,1,0,0x0004))
# Section header: name, misc, VA, size, rawptr, relocptr, lineptr, nreloc, nline, characteristics
coff.extend(struct.pack('<8sIIIIIIHHI',b'.rsrc\0\0\0',0,0,raw_size,raw_ptr,reloc_ptr,0,len(relocs),0,0xC0300040))
coff.extend(blob)
for r in relocs:
    coff.extend(struct.pack('<IIH',r,0,0x0003))
# one static section symbol
coff.extend(struct.pack('<8sIhHBB',b'.rsrc\0\0\0',0,1,0,3,0))
coff.extend(struct.pack('<I',4))
out_path.write_bytes(coff)
print(f'{out_path}: {len(coff)} bytes, icons={count}, rsrc={raw_size}, relocs={len(relocs)}')
