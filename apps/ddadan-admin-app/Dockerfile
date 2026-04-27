# syntax=docker/dockerfile:1.7

# ---- Build stage ----
FROM node:20-alpine AS build
WORKDIR /repo

COPY package.json package-lock.json ./
COPY apps/ddadan-api-server/package.json ./apps/ddadan-api-server/
COPY apps/ddadan-admin-app/package.json ./apps/ddadan-admin-app/
COPY apps/ddadan-client-app/package.json ./apps/ddadan-client-app/
COPY services/ddadan-service-pi/package.json ./services/ddadan-service-pi/

RUN npm ci

COPY apps/ddadan-admin-app ./apps/ddadan-admin-app
RUN npm run build --workspace ddadan-admin-app

# ---- Runtime stage ----
FROM nginx:1.27-alpine AS runtime
COPY apps/ddadan-admin-app/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /repo/apps/ddadan-admin-app/dist/ddadan-admin-app/browser /usr/share/nginx/html
EXPOSE 80
