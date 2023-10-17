FROM rustlang/rust:nightly

WORKDIR /project
COPY . .

RUN cargo update
RUN apt update && apt install sqlite3
RUN cargo build --release --bin mgmtd

RUN echo "shared_secret" > /auth_file
RUN mkdir -p /var/lib/beegfs

WORKDIR /project/target/release
