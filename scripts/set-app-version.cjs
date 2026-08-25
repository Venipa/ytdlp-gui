// Writes APP_VERSION into package.json without reformatting the file.
const fs = require("node:fs");

const version = process.env.APP_VERSION;
if (!version) {
	console.error("APP_VERSION is not set");
	process.exit(1);
}

const file = "package.json";
const text = fs.readFileSync(file, "utf8");
const next = text.replace(/("version"\s*:\s*")[^"]*(")/, `$1${version}$2`);
if (next === text) {
	console.error(`failed to replace version field in ${file}`);
	process.exit(1);
}
fs.writeFileSync(file, next);
console.log(`package.json version set to ${version}`);
