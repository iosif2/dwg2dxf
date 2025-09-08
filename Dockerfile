FROM ubuntu:22.04 AS libredwg-installer

RUN apt-get update && apt-get install -y \
    git \
    gcc \
    make \
    autoconf \
    automake \
    libtool \
    texinfo

RUN git clone https://github.com/LibreDWG/libredwg.git --depth=1

WORKDIR /libredwg

RUN sh autogen.sh 
RUN ./configure --disable-bindings --disable-python
RUN make -j
RUN make install

FROM rust:1.89.0 AS rust-builder

WORKDIR /usr/src/dwg2dxf

COPY . .

RUN cargo build --release

FROM ubuntu:24.04

RUN apt-get update && apt-get install -y libc6

COPY --from=libredwg-installer /usr/local/bin/* /usr/local/bin/
COPY --from=libredwg-installer /usr/local/lib/* /usr/local/lib/
COPY --from=libredwg-installer /usr/local/share/* /usr/local/share/
COPY --from=libredwg-installer /usr/local/include/* /usr/local/include/

COPY --from=rust-builder /usr/src/dwg2dxf/target/release/dwg2dxf-api /usr/local/bin/dwg2dxf-api

EXPOSE 3000

ENV LD_LIBRARY_PATH=/usr/local/lib:$LD_LIBRARY_PATH

CMD [ "dwg2dxf-api" ]