import * as fs from "fs/promises";
import * as path from "path";
import * as os from "os";
import { generateRefundKey, refundKeyFile } from "../src/refund-key.ts";

// web/scripts -> repo root. The key must not land in git history.
const repoRoot = path.resolve(import.meta.dirname, "../..");

const requested = process.env.VACA_SERVER_KEY_PATH?.trim();
const target = requested
	? path.resolve(requested)
	: path.join(os.homedir(), ".vaca", "server-key.txt");

if (isInsideRepo(target)) {
	console.error("refusing: server key path is inside the repo");
	process.exit(1);
}

try {
	await fs.stat(target);
	console.error("refusing: server key file already exists");
	process.exit(1);
} catch (err) {
	if (!isEnoent(err)) throw err;
}

const directory = path.dirname(target);
await fs.mkdir(directory, { recursive: true, mode: 0o700 });
await fs.chmod(directory, 0o700);

const key = generateRefundKey();
await fs.writeFile(target, `${refundKeyFile(key)}\n`, { mode: 0o600 });
await fs.chmod(target, 0o600);

console.log("wrote server key");
console.log(key.pubkey);

function isInsideRepo(filePath: string): boolean {
	const relative = path.relative(repoRoot, filePath);
	return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function isEnoent(err: unknown): boolean {
	return typeof err === "object" && err !== null && "code" in err && err.code === "ENOENT";
}
