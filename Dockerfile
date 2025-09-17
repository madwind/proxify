FROM golang:1.25.1-trixie AS builder

WORKDIR /app

COPY . .

ARG BUILDTIME
ARG TARGETOS
ARG TARGETARCH

RUN CGO_ENABLED=0 \
    go build -o proxify -ldflags="-s -w" .

FROM debian:stable-slim

WORKDIR /app
COPY --from=builder /app/proxify .

ENV PROXY_PATH=/proxy
ENV PORT=8080

EXPOSE 8080

CMD ["./proxify"]
