"""Reconstruct ELF import relocations in the one fingerprinted local snapshot.

The snapshot retains a packed table of named imports, but its live GOT contains
addresses from the machine where it was dumped. Turn those records into normal
ELF64 dynamic relocations so the host linker binds them for the current process.
No instruction bytes or security checks are changed.
"""
import hashlib
import struct

SOURCE_SHA256 = "663b147927e7b9d60cef7bd41cf49d3fd64058619880391083ea1ba872d6be3b"


def elf_hash(name):
    result = 0
    for byte in name:
        result = (result << 4) + byte
        high = result & 0xF0000000
        if high:
            result ^= high >> 24
        result &= ~high
    return result


def prepare(source):
    if hashlib.sha256(source).hexdigest() != SOURCE_SHA256:
        raise ValueError("Unknown Minecraft build; refusing import reconstruction")
    data = bytearray(source)
    strings = bytearray(source[0x44C0:0x44C0 + 3131])
    symbols = bytearray(source[0x458:0x458 + 214 * 24])
    versions = bytearray(source[0x7850:0x7850 + 214 * 2])
    relocations = bytearray(source[0x7FD0:0x7FD0 + 384])
    names = {}
    cursor = 0
    while cursor < len(strings):
        end = strings.index(0, cursor)
        names[bytes(strings[cursor:end])] = cursor
        cursor = end + 1
    source_strings = source[0x1B93984:0x1B980CC]
    destinations = set()
    for offset in range(0x1B980CC, 0x1B9DD78, 36):
        kind, name, version, destination, addend, weak, size = struct.unpack_from('<IIIQQII', source, offset)
        if kind not in (0, 6, 7) or addend or size or weak not in (0, 1):
            raise ValueError("Unexpected import record")
        if not 0x16C7A80 <= destination <= 0x16C8FE0 or destination in destinations:
            raise ValueError("Unexpected or duplicate import destination")
        destinations.add(destination)
        if not 0 < name < len(source_strings) or not 0 <= version < len(source_strings):
            raise ValueError("Invalid import name")
        symbol = source_strings[name:source_strings.index(0, name)]
        symbol.decode('ascii')
        if symbol not in names:
            names[symbol] = len(strings)
            strings += symbol + b'\0'
        index = len(symbols) // 24
        symbols += struct.pack('<IBBHQQ', names[symbol], 0x20 if weak else 0x10, 0, 0, 0, 0)
        versions += struct.pack('<H', 1)
        relocations += struct.pack('<QQq', destination, (index << 32) | 6, 0)
    if len(destinations) != 659:
        raise ValueError("Import count mismatch")
    count = len(symbols) // 24
    bucket_count = 521
    buckets = [0] * bucket_count
    chains = [0] * count
    for index in range(1, count):
        start = struct.unpack_from('<I', symbols, index * 24)[0]
        name = strings[start:strings.index(0, start)]
        bucket = elf_hash(name) % bucket_count
        chains[index] = buckets[bucket]
        buckets[bucket] = index
    hashes = struct.pack('<II', bucket_count, count) + struct.pack('<' + 'I' * bucket_count, *buckets) + struct.pack('<' + 'I' * count, *chains)
    new_offset = (len(data) + 4095) & ~4095
    new_address = 0x3000000
    data += bytes(new_offset - len(data))
    addresses = {}
    for tag, blob in ((5, strings), (6, symbols), (4, hashes), (0x6FFFFFF0, versions), (7, relocations)):
        data += bytes((-len(data)) % 8)
        addresses[tag] = new_address + len(data) - new_offset
        data += blob
    addresses[10] = len(strings)
    addresses[8] = len(relocations)
    for offset in range(0x12C77A8, 0x12C77A8 + 0x290, 16):
        tag, value = struct.unpack_from('<QQ', data, offset)
        if tag in addresses:
            struct.pack_into('<Q', data, offset + 8, addresses[tag])
    # The build-ID NOTE is expendable; reuse its header for the new data LOAD.
    note = 64 + 4 * 56
    if struct.unpack_from('<I', data, note)[0] != 4:
        raise ValueError("Unexpected program-header layout")
    size = len(data) - new_offset
    struct.pack_into('<IIQQQQQQ', data, note, 1, 6, new_offset, new_address, new_address, size, size, 4096)
    return bytes(data)
