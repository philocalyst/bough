import sys, zlib, struct
def write_png(path, w, h, rgba):
    raw = b''.join(b'\x00' + rgba[y*w*4:(y+1)*w*4] for y in range(h))
    def chunk(t, d):
        c = struct.pack('>I', len(d)) + t + d
        return c + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    open(path, 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 3)) + chunk(b'IEND', b''))
src, dst, w, h = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
bgra = len(sys.argv) > 5 and sys.argv[5] == 'bgra'
d = open(src, 'rb').read()
if bgra:
    b = bytearray(d); b[0::4], b[2::4] = d[2::4], d[0::4]; d = bytes(b)
write_png(dst, w, h, d)
