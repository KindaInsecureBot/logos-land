BINARY := methods/guest/target/riscv32im-risc0-zkvm-elf/release/land_registry.bin
IDL := land-registry-idl.json

.PHONY: build idl cli deploy setup inspect status clean

build:
	RISC0_DEV_MODE=1 cargo build --release

idl:
	cargo run --bin generate_idl > $(IDL)

cli:
	cargo run --bin land_registry_cli -- -p $(BINARY) --idl $(IDL) $(ARGS)

deploy:
	cargo run --bin land_registry_cli -- deploy -p $(BINARY)

setup:
	cargo run --bin land_registry_cli -- setup

inspect:
	cargo run --bin land_registry_cli -- inspect -p $(BINARY)

status:
	cargo run --bin land_registry_cli -- status -p $(BINARY)

clean:
	cargo clean
