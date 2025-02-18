import { base64 } from "@shared/base64";
import { logger } from "@shared/logger";
import { generateRandom } from "@shared/randomString";
import slugify, { SlugifyOptions } from "@shared/slug";
import { app } from "electron";
import { ConfOptions as Options, Conf as Store } from "electron-conf/main";
import Encryption from "encryption.js";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { parse as deserialize, stringify as serialize } from "yaml";
const slugifyOptions = {
  lower: true,
  replacement: "_",
  trim: true,
  remove: /[*+~.()'"!:@]/g,
} as SlugifyOptions;
const getStoreUserData = () => app.getPath("userData");
if (!statSync(getStoreUserData(), { throwIfNoEntry: false }))
  mkdirSync(getStoreUserData(), { recursive: true });
export const createYmlStore = <T extends Record<string, any> = Record<string, any>>(
  name: string,
  options: Options<T> = {} as Options<T>,
) =>
  new Store<T>({
    ext: ".yml",
    ...options,
    serializer: {
      read(raw) {
        return deserialize(raw);
      },
      write(value) {
        return serialize(value);
      },
    },
    name,
  });

export const createEncryptedStore = <T extends Record<string, any> = Record<string, any>>(
  name: string,
  options: Options<T> = {} as Options<T>,
) => {
  const encryptionKeyPath = path.join(getStoreUserData(), slugify(name, slugifyOptions) + ".key");
  const secret = base64.encode(name + "_app_secret_salt");
  logger.debug(name, {secret})
  const enc = new Encryption({ secret: secret });
  if (!existsSync(encryptionKeyPath)) writeFileSync(encryptionKeyPath, enc.encrypt({ name, secret: generateRandom(32) }));
  const encryptionKey = readFileSync(encryptionKeyPath).toString("utf8");
  const payload = enc.decrypt<{ name: string; secret: string }>(encryptionKey);
  if (!payload || name !== payload?.name) throw new Error("Invalid encryption key");
  if (!payload.secret) throw new Error("Invalid encryption secret");
  const storeEncryptor = new Encryption({ secret: payload.secret });
  return new Store<T>({
    ext: ".data",
    ...options,
    serializer: {
      read(raw) {
        return storeEncryptor.decrypt(raw) as T;
      },
      write(value) {
        return storeEncryptor.encrypt(value);
      },
    },
    name,
  });
};
