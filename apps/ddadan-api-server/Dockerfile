# syntax=docker/dockerfile:1.7

# ---- Build stage ----
FROM node:20-alpine AS build
WORKDIR /repo

RUN apk add --no-cache python3 make g++

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
ENV PORT=7800
ENV DB_PATH=/app/data/ddadan.sqlite
ENV ASSETS_DIR=/app/storage/assets

RUN apk add --no-cache tini && \
    mkdir -p /app/storage/assets /app/data

COPY --from=build /repo/node_modules ./node_modules
COPY --from=build /repo/apps/ddadan-api-server/dist ./dist
COPY --from=build /repo/apps/ddadan-api-server/package.json ./package.json

COPY apps/ddadan-api-server/docker-entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 7800
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD wget -q -O /dev/null http://127.0.0.1:7800/api/health/live || exit 1
ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/entrypoint.sh"]
CMD ["node", "dist/main.js"]
