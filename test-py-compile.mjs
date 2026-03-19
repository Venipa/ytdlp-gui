// test-py-compile.mjs - run with: node test-py-compile.mjs
import { execSync } from "node:child_process";
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, resolve } from "path";

const workerPy = resolve("src/main/lib/ytdlp-service/worker.py");
const outFile = resolve(tmpdir(), `ytdlp-${basename(workerPy, ".py")}-${Date.now()}.pyc`);
const scriptPath = resolve(tmpdir(), "ytdlp-compile-worker.py");
const compileScript = "import py_compile\nimport sys\npy_compile.compile(sys.argv[1], cfile=sys.argv[2])\n";

console.log("Testing Python bytecode compilation...");
console.log("  Source:", workerPy);
console.log("  Output:", outFile);
console.log("  Source exists:", existsSync(workerPy));

writeFileSync(scriptPath, compileScript);
try {
  for (const py of ["python", "python3", "py"]) {
    try {
      console.log(`  Trying: ${py}...`);
      execSync(`${py} "${scriptPath}" "${workerPy}" "${outFile}"`, { stdio: "inherit" });
      console.log("  Success!");
      break;
    } catch (e) {
      console.log(`  ${py} failed:`, e.message);
    }
  }
  if (existsSync(outFile)) {
    const buf = readFileSync(outFile);
    console.log("  .pyc size:", buf.length, "bytes");
  } else {
    console.log("  FAILED: .pyc was not created");
  }
} finally {
  unlinkSync(scriptPath);
  if (existsSync(outFile)) unlinkSync(outFile);
}
