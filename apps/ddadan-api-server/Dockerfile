# syntax=docker/dockerfile:1.7

# ---- Build stage ----
FROM node:20-alpine AS build
WORKDIR /repo

# Copy lockfile + every workspace's package.json so npm ci can resolve the tree.
COPY package.json package-lock.json ./
COPY apps/ddadan-api-server/package.json ./apps/ddadan-api-server/
COPY apps/ddadan-admin-app/package.json ./apps/ddadan-admin-app/
COPY apps/ddadan-client-app/package.json ./apps/ddadan-client-app/
COPY services/ddadan-service-pi/package.json ./services/ddadan-service-pi/

RUN npm ci

# Copy api source and build
COPY apps/ddadan-api-server ./apps/ddadan-api-server
RUN npm run build --workspace ddadan-api-server

# ---- Runtime stage ----
FROM node:20-alpine AS runtime
WORKDIR /app
ENV NODE_ENV=production

RUN apk add --no-cache tini && \
    mkdir -p /app/storage/assets /app/firebase

COPY --from=build /repo/node_modules ./node_modules
COPY --from=build /repo/apps/ddadan-api-server/dist ./dist
COPY --from=build /repo/apps/ddadan-api-server/package.json ./package.json

COPY apps/ddadan-api-server/docker-entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 3000
ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/entrypoint.sh"]
CMD ["node", "dist/main.js"]
