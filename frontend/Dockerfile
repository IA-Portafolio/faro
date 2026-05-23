FROM node:20-alpine AS builder
WORKDIR /app

ARG PUBLIC_API_BASE=http://localhost:8080
ENV PUBLIC_API_BASE=$PUBLIC_API_BASE

COPY package.json ./
RUN npm install --no-audit --no-fund

COPY . .
RUN npm run build

FROM node:20-alpine
WORKDIR /app
RUN apk add --no-cache wget

COPY --from=builder /app/build ./build
COPY --from=builder /app/package.json ./
COPY --from=builder /app/node_modules ./node_modules

ENV NODE_ENV=production
ENV PORT=3000
EXPOSE 3000

CMD ["node", "build"]
