export const environment = {
  production: false,
  firebase: {
    apiKey: "ddadan-dev-emulator-key",
    authDomain: "ddadan-dev.firebaseapp.com",
    projectId: "ddadan-dev",
    appId: "ddadan-dev-consumer",
  },
  authEmulator: { enabled: true, useSameOrigin: true },
} as const;
