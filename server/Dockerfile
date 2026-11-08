# Tatakai extension server, standalone Linux image.
# Build from the REPOSITORY ROOT (root package.json + lockfile, app sources bundled in place):
#   docker build -f server/Dockerfile -t tatakai-server .
#   docker run --rm -p 8787:8787 -v tatakai-data:/data tatakai-server
FROM node:24-bookworm-slim AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts --no-audit --no-fund
# Only what server/scripts/build.mjs reads: the server, the app code it bundles
# (interpreter.ts -> src/core/extensions/{sora,mangayomi,compatibility}; dashboard ->
# src/components/ui, src/lib/utils, src/locales) and the compatibility runtime.
# If the dashboard starts importing more of '@/...', add those paths here and in .dockerignore.
COPY server ./server
COPY src/core/extensions ./src/core/extensions
COPY src/components/ui ./src/components/ui
COPY src/lib ./src/lib
COPY src/locales ./src/locales
COPY desktop/runtime/compatibility ./desktop/runtime/compatibility
COPY shared/aniyomi ./shared/aniyomi
RUN npm run server:build

FROM node:24-bookworm-slim
ENV NODE_ENV=production HOST=0.0.0.0 PORT=8787 DATA_DIR=/data
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts --no-audit --no-fund && npm cache clean --force
COPY --from=build /app/server/src ./server/src
COPY --from=build /app/server/dist ./server/dist
COPY --from=build /app/server/LICENSE /app/server/THIRD_PARTY.md /app/server/README.md ./server/
# The Java compatibility broker is required in place; aniyomi.cjs verifies its
# bootstrap sources at desktop/runtime/compatibility/aniyomi-java and shared/aniyomi.
COPY --from=build /app/desktop/runtime/compatibility ./desktop/runtime/compatibility
COPY --from=build /app/shared/aniyomi ./shared/aniyomi
RUN mkdir /data && chown node:node /data
USER node
VOLUME /data
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s CMD ["node", "-e", "fetch('http://127.0.0.1:'+(process.env.PORT||8787)+'/api/server/v1/health').then(r=>process.exit(r.ok?0:1),()=>process.exit(1))"]
CMD ["node", "server/src/node.mjs"]
