import { spawn, spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "../../..");
const certificateDirectory = resolve(
  repositoryRoot,
  ".angular/coupon-dev-certs",
);
const certificatePaths = {
  authorityCertificate: resolve(certificateDirectory, "coupon-dev-ca.crt"),
  authorityKey: resolve(certificateDirectory, "coupon-dev-ca.key.pem"),
  iosProfile: resolve(certificateDirectory, "coupon-dev-ca.mobileconfig"),
  serverCertificate: resolve(certificateDirectory, "coupon-lan.crt.pem"),
  serverKey: resolve(certificateDirectory, "coupon-lan.key.pem"),
};

const applications = new Set([
  "coupon-consumer-app",
  "coupon-store-app",
  "coupon-system-admin-app",
]);

function runOpenSsl(arguments_) {
  const result = spawnSync("openssl", arguments_, {
    cwd: repositoryRoot,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(
      `openssl ${arguments_[0]} failed: ${result.stderr || result.stdout}`,
    );
  }
}

function certificateExists() {
  return [
    certificatePaths.authorityCertificate,
    certificatePaths.authorityKey,
    certificatePaths.serverCertificate,
    certificatePaths.serverKey,
  ].every((path) => existsSync(path));
}

function generateIosProfile() {
  const certificateData = readFileSync(
    certificatePaths.authorityCertificate,
    "utf8",
  )
    .replace("-----BEGIN CERTIFICATE-----", "")
    .replace("-----END CERTIFICATE-----", "")
    .replaceAll(/\s/g, "");

  writeFileSync(
    certificatePaths.iosProfile,
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>PayloadCertificateFileName</key>
      <string>coupon-dev-ca.crt</string>
      <key>PayloadContent</key>
      <data>${certificateData}</data>
      <key>PayloadDescription</key>
      <string>Trusts the DDADAN coupon LAN development server.</string>
      <key>PayloadDisplayName</key>
      <string>DDADAN Coupon Development CA</string>
      <key>PayloadIdentifier</key>
      <string>kr.ddadan.coupon.dev-ca.certificate</string>
      <key>PayloadType</key>
      <string>com.apple.security.root</string>
      <key>PayloadUUID</key>
      <string>92E88A3E-41B0-4E01-B648-8820D8981828</string>
      <key>PayloadVersion</key>
      <integer>1</integer>
    </dict>
  </array>
  <key>PayloadDescription</key>
  <string>Installs the root CA used by DDADAN coupon LAN development servers.</string>
  <key>PayloadDisplayName</key>
  <string>DDADAN Coupon Development CA</string>
  <key>PayloadIdentifier</key>
  <string>kr.ddadan.coupon.dev-ca</string>
  <key>PayloadOrganization</key>
  <string>DDADAN</string>
  <key>PayloadRemovalDisallowed</key>
  <false/>
  <key>PayloadType</key>
  <string>Configuration</string>
  <key>PayloadUUID</key>
  <string>8612B970-CFD0-4895-849B-C812305F4E15</string>
  <key>PayloadVersion</key>
  <integer>1</integer>
</dict>
</plist>
`,
    { mode: 0o644 },
  );
}

function generateCertificate() {
  mkdirSync(certificateDirectory, { recursive: true, mode: 0o700 });
  if (certificateExists()) {
    generateIosProfile();
    return;
  }

  const requestPath = resolve(certificateDirectory, "coupon-lan.csr.pem");
  const extensionsPath = resolve(certificateDirectory, "coupon-lan.ext");

  runOpenSsl(["genrsa", "-out", certificatePaths.authorityKey, "2048"]);
  runOpenSsl([
    "req",
    "-x509",
    "-new",
    "-sha256",
    "-key",
    certificatePaths.authorityKey,
    "-days",
    "3650",
    "-out",
    certificatePaths.authorityCertificate,
    "-subj",
    "/CN=DDADAN Coupon Local Development CA",
    "-addext",
    "basicConstraints=critical,CA:TRUE",
    "-addext",
    "keyUsage=critical,keyCertSign,cRLSign",
  ]);
  runOpenSsl([
    "req",
    "-new",
    "-newkey",
    "rsa:2048",
    "-nodes",
    "-sha256",
    "-keyout",
    certificatePaths.serverKey,
    "-out",
    requestPath,
    "-subj",
    "/CN=192.168.150.185",
  ]);

  writeFileSync(
    extensionsPath,
    [
      "basicConstraints=critical,CA:FALSE",
      "keyUsage=critical,digitalSignature,keyEncipherment",
      "extendedKeyUsage=serverAuth",
      "subjectAltName=IP:192.168.150.185,IP:127.0.0.1,DNS:localhost",
      "",
    ].join("\n"),
    { mode: 0o600 },
  );
  runOpenSsl([
    "x509",
    "-req",
    "-sha256",
    "-in",
    requestPath,
    "-CA",
    certificatePaths.authorityCertificate,
    "-CAkey",
    certificatePaths.authorityKey,
    "-CAcreateserial",
    "-days",
    "365",
    "-out",
    certificatePaths.serverCertificate,
    "-extfile",
    extensionsPath,
  ]);

  rmSync(requestPath, { force: true });
  rmSync(extensionsPath, { force: true });
  chmodSync(certificatePaths.authorityKey, 0o600);
  chmodSync(certificatePaths.serverKey, 0o600);
  chmodSync(certificatePaths.authorityCertificate, 0o644);
  chmodSync(certificatePaths.serverCertificate, 0o644);
  generateIosProfile();
}

function printCertificateInstructions() {
  console.log("Coupon LAN HTTPS certificate is ready.");
  console.log(`  Root CA: ${certificatePaths.authorityCertificate}`);
  console.log(`  iOS    : ${certificatePaths.iosProfile}`);
  console.log(`  Server : ${certificatePaths.serverCertificate}`);
  console.log("Install and fully trust the Root CA on each test device.");
}

function serve(application) {
  if (!applications.has(application)) {
    throw new Error(`Unknown coupon application: ${application}`);
  }

  generateCertificate();
  printCertificateInstructions();

  const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";
  const child = spawn(
    npmCommand,
    [
      "run",
      "start",
      "--workspace",
      application,
      "--",
      "--host",
      "0.0.0.0",
      "--ssl",
      "--ssl-cert",
      certificatePaths.serverCertificate,
      "--ssl-key",
      certificatePaths.serverKey,
    ],
    { cwd: repositoryRoot, stdio: "inherit" },
  );

  child.once("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exitCode = code ?? 1;
  });
}

const [command, application] = process.argv.slice(2);

if (command === "certificate") {
  generateCertificate();
  printCertificateInstructions();
} else if (command === "serve" && application) {
  serve(application);
} else {
  console.error(
    "Usage: coupon-lan-dev.mjs certificate | serve <coupon application>",
  );
  process.exitCode = 2;
}
