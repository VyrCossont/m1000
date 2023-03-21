FROM alpine:latest

# https://github.com/opencontainers/image-spec/blob/main/annotations.md
LABEL 'org.opencontainers.image.authors'='Vyr Cossont'
LABEL 'org.opencontainers.image.url'='https://catgirl.codes/m1000'
LABEL 'org.opencontainers.image.source'='https://github.com/VyrCossont/m1000'
LABEL 'org.opencontainers.image.version'='0.1.0'
LABEL 'org.opencontainers.image.licenses'='MIT'

RUN apk --update-cache upgrade

ARG RUST_PROFILE='release-max'
ARG RUST_TARGET_TRIPLE='x86_64-unknown-linux-musl'
COPY /target/${RUST_TARGET_TRIPLE}/${RUST_PROFILE}/m1000 /usr/bin/

VOLUME /config
EXPOSE 1337/tcp
USER 1000:1000

ENTRYPOINT ["/usr/bin/m1000", "/config"]
HEALTHCHECK CMD ["healthcheck"]
CMD ["serve"]
