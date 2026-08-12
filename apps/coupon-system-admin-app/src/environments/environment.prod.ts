export const environment = {
  production: true,
  firebase: {
    apiKey: "replace-with-admin-audience-key",
    authDomain: "replace-with-admin-audience-domain",
    projectId: "replace-with-admin-audience-project",
    appId: "replace-with-admin-audience-app",
  },
  authEmulator: { enabled: false, useSameOrigin: false },
} as const;
