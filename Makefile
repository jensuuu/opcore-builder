.PHONY: build linux windows

build:
	./package.sh

linux:
	./package.sh linux

windows:
	./package.sh windows