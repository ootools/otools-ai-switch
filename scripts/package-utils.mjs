import fs from 'node:fs';
import path from 'node:path';
import { crc32, deflateRawSync, inflateRawSync } from 'node:zlib';

export function option(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index < 0 ? fallback : process.argv[index + 1] || fallback;
}

export function validPath(name) {
  return typeof name === 'string' && name.length > 0 && !/[\\:\x00]/.test(name)
    && !name.split('/').some((part) => !part || part === '.' || part === '..');
}

export function validateManifest(manifest) {
  if (!/^[a-z0-9][a-z0-9._-]*$/i.test(manifest.packid || '')) throw new Error('Invalid packid');
  if (manifest.uuid !== manifest.packid) throw new Error('uuid must match packid');
  if (!/^\d+\.\d+\.\d+(?:-[a-z0-9.-]+)?$/i.test(manifest.version || '')) throw new Error('Invalid version');
  if (!validPath(manifest.entry)) throw new Error('Invalid plugin entry path');
  if (manifest.openMode !== 'panel') throw new Error('AI Switch must default to panel mode');
  if (!manifest.native?.enabled) throw new Error('AI Switch requires its native library');
}

export function walk(root, relative = '') {
  const result = [];
  for (const name of fs.readdirSync(path.join(root, relative)).sort()) {
    const entry = relative ? `${relative}/${name}` : name;
    const full = path.join(root, entry);
    const stat = fs.lstatSync(full);
    if (stat.isSymbolicLink()) throw new Error(`Symbolic links cannot be packaged: ${entry}`);
    if (stat.isDirectory()) result.push(...walk(root, entry));
    else if (stat.isFile()) result.push(entry);
  }
  return result;
}

export function writeZip(entries) {
  const local = [];
  const central = [];
  let offset = 0;
  for (const { name, data } of entries) {
    if (!validPath(name)) throw new Error(`Invalid archive path: ${name}`);
    const nameBytes = Buffer.from(name);
    const compressed = deflateRawSync(data, { level: 9 });
    const payload = compressed.length < data.length ? compressed : data;
    const method = payload === compressed ? 8 : 0;
    const checksum = crc32(data);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(0x0800, 6);
    header.writeUInt16LE(method, 8);
    header.writeUInt16LE(0x21, 12); // Stable ZIP epoch: 1980-01-01.
    header.writeUInt32LE(checksum, 14);
    header.writeUInt32LE(payload.length, 18);
    header.writeUInt32LE(data.length, 22);
    header.writeUInt16LE(nameBytes.length, 26);
    local.push(header, nameBytes, payload);
    const directory = Buffer.alloc(46);
    directory.writeUInt32LE(0x02014b50, 0);
    directory.writeUInt16LE(20, 4);
    directory.writeUInt16LE(20, 6);
    directory.writeUInt16LE(0x0800, 8);
    directory.writeUInt16LE(method, 10);
    directory.writeUInt16LE(0x21, 14);
    directory.writeUInt32LE(checksum, 16);
    directory.writeUInt32LE(payload.length, 20);
    directory.writeUInt32LE(data.length, 24);
    directory.writeUInt16LE(nameBytes.length, 28);
    directory.writeUInt32LE(offset, 42);
    central.push(directory, nameBytes);
    offset += header.length + nameBytes.length + payload.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, directory, end]);
}

export function readZip(buffer) {
  const end = buffer.length - 22;
  if (end < 0 || buffer.readUInt32LE(end) !== 0x06054b50) throw new Error('Invalid ZIP end record');
  const count = buffer.readUInt16LE(end + 10);
  const centralSize = buffer.readUInt32LE(end + 12);
  let offset = buffer.readUInt32LE(end + 16);
  if (offset + centralSize !== end) throw new Error('Invalid ZIP directory size');
  const entries = new Map();
  for (let i = 0; i < count; i++) {
    if (buffer.readUInt32LE(offset) !== 0x02014b50) throw new Error('Invalid ZIP central header');
    const method = buffer.readUInt16LE(offset + 10);
    const checksum = buffer.readUInt32LE(offset + 16);
    const compressedSize = buffer.readUInt32LE(offset + 20);
    const size = buffer.readUInt32LE(offset + 24);
    const nameSize = buffer.readUInt16LE(offset + 28);
    const extraSize = buffer.readUInt16LE(offset + 30);
    const commentSize = buffer.readUInt16LE(offset + 32);
    const local = buffer.readUInt32LE(offset + 42);
    const name = buffer.toString('utf8', offset + 46, offset + 46 + nameSize);
    if (!validPath(name) || entries.has(name)) throw new Error(`Invalid or duplicate ZIP path: ${name}`);
    if (buffer.readUInt32LE(local) !== 0x04034b50) throw new Error(`Invalid local header: ${name}`);
    const localNameSize = buffer.readUInt16LE(local + 26);
    if (buffer.toString('utf8', local + 30, local + 30 + localNameSize) !== name) throw new Error('ZIP filename mismatch');
    if (buffer.readUInt16LE(local + 8) !== method) throw new Error('ZIP compression mismatch');
    const start = local + 30 + localNameSize + buffer.readUInt16LE(local + 28);
    const payload = buffer.subarray(start, start + compressedSize);
    if (payload.length !== compressedSize) throw new Error(`Truncated ZIP entry: ${name}`);
    if (method !== 0 && method !== 8) throw new Error(`Unsupported compression: ${method}`);
    const data = method === 8 ? inflateRawSync(payload) : payload;
    if (data.length !== size || crc32(data) !== checksum) throw new Error(`ZIP CRC/size mismatch: ${name}`);
    entries.set(name, data);
    offset += 46 + nameSize + extraSize + commentSize;
  }
  if (offset !== end) throw new Error('ZIP directory did not terminate correctly');
  return entries;
}

export function validateEntries(manifest, entries) {
  for (const required of [manifest.entry, 'logo.svg', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'UPSTREAM.md']) {
    if (!entries.has(required)) throw new Error(`Missing packaged file: ${required}`);
  }
  const html = entries.get(manifest.entry).toString('utf8');
  for (const tag of html.matchAll(/<(?:script|link|img)\b[^>]*>/gi)) {
    const reference = /\b(?:src|href)\s*=\s*["']([^"']+)["']/i.exec(tag[0])?.[1];
    if (!reference || reference.startsWith('data:')) continue;
    if (reference.startsWith('/') || /^[a-z][a-z0-9+.-]*:/i.test(reference)) {
      throw new Error(`Frontend asset must be package-relative: ${reference}`);
    }
    const asset = path.posix.normalize(path.posix.join(path.posix.dirname(manifest.entry), reference.split(/[?#]/)[0]));
    if (!validPath(asset) || !entries.has(asset)) throw new Error(`Missing frontend asset: ${reference}`);
  }
  const libraries = [...entries.keys()].filter((name) => /^lib\/(Windows\.dll|macOS\.dylib|Linux\.so)$/.test(name));
  if (!libraries.length) throw new Error('Missing native library; run native:build before packaging');
  for (const name of libraries) {
    const data = entries.get(name);
    for (const symbol of ['otools_plugin_invoke', 'otools_plugin_free']) {
      if (!data.includes(Buffer.from(symbol))) throw new Error(`Missing ${symbol} in native library ${name}`);
    }
  }
  return libraries;
}
