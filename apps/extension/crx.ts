// Hand-written CRX3 packing (no dependency): zip + CrxFileHeader protobuf + RSA-SHA256 signature.
import { createHash, createSign, type KeyObject } from "node:crypto";
import { deflateRawSync } from "node:zlib";

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
export function crc32(b: Uint8Array): number {
  let c = 0xffffffff;
  for (const x of b) c = crcTable[(c ^ x) & 0xff]! ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

const le = (n: number, bytes: 2 | 4) => {
  const b = Buffer.alloc(bytes);
  bytes === 2 ? b.writeUInt16LE(n) : b.writeUInt32LE(n);
  return b;
};

/** A zip of `files` (path -> bytes), deflated, with a fixed timestamp so builds are reproducible. */
export function zip(files: Map<string, Uint8Array>): Buffer {
  const locals: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  const time = 0, date = (2026 - 1980) << 9 | 1 << 5 | 1;
  for (const [name, data] of [...files].sort(([a], [b]) => (a < b ? -1 : 1))) {
    const n = Buffer.from(name);
    const raw = Buffer.from(data);
    const def = deflateRawSync(raw);
    const crc = crc32(raw);
    const head = Buffer.concat([le(0x04034b50, 4), le(20, 2), le(0x0800, 2), le(8, 2), le(time, 2), le(date, 2), le(crc, 4), le(def.length, 4), le(raw.length, 4), le(n.length, 2), le(0, 2), n]);
    locals.push(head, def);
    central.push(Buffer.concat([le(0x02014b50, 4), le(20, 2), le(20, 2), le(0x0800, 2), le(8, 2), le(time, 2), le(date, 2), le(crc, 4), le(def.length, 4), le(raw.length, 4), le(n.length, 2), le(0, 2), le(0, 2), le(0, 2), le(0, 2), le(0, 4), le(offset, 4), n]));
    offset += head.length + def.length;
  }
  const cd = Buffer.concat(central);
  const end = Buffer.concat([le(0x06054b50, 4), le(0, 2), le(0, 2), le(central.length, 2), le(central.length, 2), le(cd.length, 4), le(offset, 4), le(0, 2)]);
  return Buffer.concat([...locals, cd, end]);
}

const varint = (n: number): Buffer => {
  const out: number[] = [];
  while (n > 0x7f) { out.push((n & 0x7f) | 0x80); n = Math.floor(n / 128); }
  out.push(n);
  return Buffer.from(out);
};
/** Protobuf length-delimited field. */
export const field = (num: number, data: Uint8Array): Buffer => Buffer.concat([varint(num * 8 + 2), varint(data.length), data]);

/** The 16-byte crx id of a public key (SPKI DER), as bytes. */
export const crxIdBytes = (spki: Uint8Array): Buffer => createHash("sha256").update(spki).digest().subarray(0, 16);

/** The 32-letter extension id (a-p) of a public key. */
export function extensionId(spki: Uint8Array): string {
  return [...crxIdBytes(spki)].map((b) => String.fromCharCode(97 + (b >> 4)) + String.fromCharCode(97 + (b & 15))).join("");
}

/** Wrap a zip into a signed CRX3 file. */
export function packCrx(zipBytes: Buffer, priv: KeyObject, spki: Buffer): Buffer {
  const signedData = field(1, crxIdBytes(spki));
  const sig = createSign("sha256")
    .update(Buffer.from("CRX3 SignedData\0"))
    .update(le(signedData.length, 4))
    .update(signedData)
    .update(zipBytes)
    .sign(priv);
  const proof = Buffer.concat([field(1, spki), field(2, sig)]);
  const header = Buffer.concat([field(2, proof), field(10000, signedData)]);
  return Buffer.concat([Buffer.from("Cr24"), le(3, 4), le(header.length, 4), header, zipBytes]);
}
