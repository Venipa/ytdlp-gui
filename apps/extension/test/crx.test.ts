import { expect, test } from "bun:test";
import { createPublicKey, generateKeyPairSync, createVerify } from "node:crypto";
import { inflateRawSync } from "node:zlib";
import { crc32, crxIdBytes, extensionId, packCrx, zip } from "../crx";

const { privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const spki = createPublicKey(privateKey).export({ type: "spki", format: "der" }) as Buffer;

test("crc32 matches the standard check value", () => {
  expect(crc32(Buffer.from("123456789"))).toBe(0xcbf43926);
});

test("extension id is 32 letters a-p and stable for a key", () => {
  const id = extensionId(spki);
  expect(id).toMatch(/^[a-p]{32}$/);
  expect(extensionId(spki)).toBe(id);
  expect(crxIdBytes(spki).length).toBe(16);
});

test("zip entries inflate back with a matching crc", () => {
  const files = new Map<string, Uint8Array>([["a.txt", Buffer.from("hello hello hello")], ["d/b.bin", Buffer.alloc(5000, 7)]]);
  const z = zip(files);
  const eocd = z.lastIndexOf(Buffer.from([0x50, 0x4b, 5, 6]));
  const count = z.readUInt16LE(eocd + 10);
  let at = z.readUInt32LE(eocd + 16);
  const got = new Map<string, Buffer>();
  for (let i = 0; i < count; i++) {
    const crc = z.readUInt32LE(at + 16), csize = z.readUInt32LE(at + 20), nlen = z.readUInt16LE(at + 28), xlen = z.readUInt16LE(at + 30), clen = z.readUInt16LE(at + 32), off = z.readUInt32LE(at + 42);
    const name = z.subarray(at + 46, at + 46 + nlen).toString();
    const lh = off + 30 + z.readUInt16LE(off + 26) + z.readUInt16LE(off + 28);
    const data = inflateRawSync(z.subarray(lh, lh + csize));
    expect(crc32(data)).toBe(crc);
    got.set(name, data);
    at += 46 + nlen + xlen + clen;
  }
  expect([...got.keys()]).toEqual(["a.txt", "d/b.bin"]);
  expect(got.get("a.txt")!.toString()).toBe("hello hello hello");
});

test("crx3 header carries a valid RSA-SHA256 signature over the zip", () => {
  const z = zip(new Map([["manifest.json", Buffer.from("{}")]]));
  const crx = packCrx(z, privateKey, spki);
  expect(crx.subarray(0, 4).toString()).toBe("Cr24");
  expect(crx.readUInt32LE(4)).toBe(3);
  const hlen = crx.readUInt32LE(8);
  const header = crx.subarray(12, 12 + hlen);
  expect(crx.subarray(12 + hlen).equals(z)).toBe(true);
  // header = field 2 (proof) then field 10000 (signed data); read the proof's signature back
  expect(header[0]).toBe(0x12);
  const plen = header[1]! < 0x80 ? header[1]! : (header[1]! & 0x7f) | (header[2]! << 7);
  const pstart = header[1]! < 0x80 ? 2 : 3;
  const proof = header.subarray(pstart, pstart + plen);
  const rest = header.subarray(pstart + plen);
  // signed_header_data: tag 10000<<3|2 = 80002 -> 3 byte varint
  const signedData = rest.subarray(rest.length - 18);
  expect(signedData.subarray(2).equals(crxIdBytes(spki))).toBe(true);
  const sigAt = proof.lastIndexOf(Buffer.from([0x12, 0x80, 0x02]));
  const sig = proof.subarray(sigAt + 3);
  const le = Buffer.alloc(4); le.writeUInt32LE(signedData.length);
  const ok = createVerify("sha256").update(Buffer.from("CRX3 SignedData\0")).update(le).update(signedData).update(z).verify(spki.length ? createPublicKey({ key: spki, format: "der", type: "spki" }) : privateKey, sig);
  expect(ok).toBe(true);
});
