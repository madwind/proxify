FROM scratch

WORKDIR /app
COPY --chmod=755 dist/proxify /app/proxify

ENTRYPOINT ["/app/proxify"]
