FROM golang:1.22-alpine AS build
WORKDIR /src
COPY go.mod go.sum* ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /out/control-plane ./cmd/control-plane

FROM alpine:3.20
RUN adduser -D -H app
USER app
COPY --from=build /out/control-plane /usr/local/bin/control-plane
EXPOSE 8088
ENTRYPOINT ["control-plane"]

