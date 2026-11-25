// usbdisk: userspace USB Mass Storage (Bulk-Only Transport) -> FUSE file.
// Exposes the first LUN of a USB mass-storage device as <mountpoint>/disk so
// tools like mtools can operate on it without kernel usb-storage/vfat support.
//
// Build: gcc -O2 -o usbdisk usbdisk.c $(pkg-config --cflags --libs libusb-1.0 fuse3)
// Run:   sudo ./usbdisk -s -f -o allow_other <mountpoint>   (VID:PID via USBDISK_ID=1949:0004)
#define FUSE_USE_VERSION 31
#include <fuse.h>
#include <libusb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <stdint.h>
#include <unistd.h>

static libusb_device_handle *h;
static uint8_t ep_in, ep_out, iface;
static uint32_t tag = 1, blksz = 512;
static uint64_t nblocks;

static int bulk(uint8_t ep, uint8_t *buf, int len, int *got) {
    int r = libusb_bulk_transfer(h, ep, buf, len, got, 20000);
    if (r == LIBUSB_ERROR_PIPE) { libusb_clear_halt(h, ep); r = libusb_bulk_transfer(h, ep, buf, len, got, 20000); }
    return r;
}

// Returns SCSI status (0 ok), negative on transport error.
static int scsi(const uint8_t *cdb, int cdblen, uint8_t *data, uint32_t len, int dir_in) {
    uint8_t cbw[31] = {0};
    memcpy(cbw, "USBC", 4);
    uint32_t t = tag++;
    memcpy(cbw + 4, &t, 4);
    memcpy(cbw + 8, &len, 4);
    cbw[12] = dir_in ? 0x80 : 0;
    cbw[13] = 0; cbw[14] = cdblen;
    memcpy(cbw + 15, cdb, cdblen);
    int got, r;
    if ((r = bulk(ep_out, cbw, 31, &got)) || got != 31) return -1;
    if (len) {
        r = bulk(dir_in ? ep_in : ep_out, data, len, &got);
        if (r && r != LIBUSB_ERROR_PIPE) return -2;
    }
    uint8_t csw[13];
    if ((r = bulk(ep_in, csw, 13, &got)) || got != 13 || memcmp(csw, "USBS", 4)) return -3;
    return csw[12];
}

static int rw_blocks(uint64_t lba, uint32_t cnt, uint8_t *buf, int write) {
    while (cnt) {
        uint32_t n = cnt > 128 ? 128 : cnt;
        uint8_t cdb[10] = { write ? 0x2A : 0x28, 0, lba >> 24, lba >> 16, lba >> 8, lba, 0, n >> 8, n, 0 };
        int s = -1;
        for (int tries = 0; tries < 8 && s != 0; tries++) {
            s = scsi(cdb, 10, buf, n * blksz, !write);
            if (s) { // fetch sense to clear UNIT ATTENTION etc., then back off
                uint8_t rs[6] = {0x03, 0, 0, 0, 18, 0}, sb[18];
                scsi(rs, 6, sb, 18, 1);
                usleep(250000);
            }
        }
        if (s) return -EIO;
        lba += n; cnt -= n; buf += n * blksz;
    }
    return 0;
}

static int fs_getattr(const char *p, struct stat *st, struct fuse_file_info *fi) {
    memset(st, 0, sizeof *st);
    if (!strcmp(p, "/")) { st->st_mode = S_IFDIR | 0755; st->st_nlink = 2; return 0; }
    if (!strcmp(p, "/disk")) { st->st_mode = S_IFREG | 0666; st->st_nlink = 1; st->st_size = nblocks * blksz; return 0; }
    return -ENOENT;
}
static int fs_readdir(const char *p, void *b, fuse_fill_dir_t f, off_t o, struct fuse_file_info *fi, enum fuse_readdir_flags fl) {
    f(b, ".", NULL, 0, 0); f(b, "..", NULL, 0, 0); f(b, "disk", NULL, 0, 0); return 0;
}
static int fs_open(const char *p, struct fuse_file_info *fi) { return strcmp(p, "/disk") ? -ENOENT : 0; }

static int io(char *buf, size_t sz, off_t off, int write) {
    uint64_t total = nblocks * blksz;
    if ((uint64_t)off >= total) return 0;
    if (off + sz > total) sz = total - off;
    uint64_t first = off / blksz, last = (off + sz - 1) / blksz;
    uint32_t cnt = last - first + 1;
    uint8_t *tmp = malloc((size_t)cnt * blksz);
    if (!tmp) return -ENOMEM;
    int r;
    int aligned = (off % blksz == 0) && (sz % blksz == 0);
    if (!write || !aligned) { if ((r = rw_blocks(first, cnt, tmp, 0))) { free(tmp); return r; } }
    if (write) {
        memcpy(tmp + off % blksz, buf, sz);
        r = rw_blocks(first, cnt, tmp, 1);
    } else { memcpy(buf, tmp + off % blksz, sz); r = 0; }
    free(tmp);
    return r ? r : (int)sz;
}
static int fs_read(const char *p, char *b, size_t s, off_t o, struct fuse_file_info *fi) { return io(b, s, o, 0); }
static int fs_write(const char *p, const char *b, size_t s, off_t o, struct fuse_file_info *fi) { return io((char *)b, s, o, 1); }
static int fs_trunc(const char *p, off_t s, struct fuse_file_info *fi) { return 0; }
static int fs_fsync(const char *p, int d, struct fuse_file_info *fi) {
    uint8_t cdb[10] = {0x35}; // SYNCHRONIZE CACHE(10)
    scsi(cdb, 10, NULL, 0, 0); return 0;
}
static const struct fuse_operations ops = {
    .getattr = fs_getattr, .readdir = fs_readdir, .open = fs_open,
    .read = fs_read, .write = fs_write, .truncate = fs_trunc, .fsync = fs_fsync, .flush = (void *)fs_fsync,
};

int main(int argc, char **argv) {
    unsigned vid = 0x1949, pid = 0x0004;
    if (getenv("USBDISK_ID")) sscanf(getenv("USBDISK_ID"), "%x:%x", &vid, &pid);
    if (libusb_init(NULL)) return 1;
    if (!(h = libusb_open_device_with_vid_pid(NULL, vid, pid))) { fprintf(stderr, "device %04x:%04x not found\n", vid, pid); return 1; }
    libusb_device *dev = libusb_get_device(h);
    struct libusb_config_descriptor *cfg;
    libusb_get_active_config_descriptor(dev, &cfg);
    for (int i = 0; i < cfg->bNumInterfaces; i++) {
        const struct libusb_interface_descriptor *d = &cfg->interface[i].altsetting[0];
        if (d->bInterfaceClass != 8) continue;
        iface = d->bInterfaceNumber;
        for (int e = 0; e < d->bNumEndpoints; e++) {
            const struct libusb_endpoint_descriptor *ep = &d->endpoint[e];
            if ((ep->bmAttributes & 3) != 2) continue;
            if (ep->bEndpointAddress & 0x80) ep_in = ep->bEndpointAddress; else ep_out = ep->bEndpointAddress;
        }
    }
    libusb_set_auto_detach_kernel_driver(h, 1);
    if (libusb_claim_interface(h, iface)) { fprintf(stderr, "claim failed\n"); return 1; }
    uint8_t tur[6] = {0}, buf[64];
    int s = -1;
    for (int i = 0; i < 10 && s != 0; i++) {
        s = scsi(tur, 6, NULL, 0, 0);
        if (s) { uint8_t rs[6] = {0x03, 0, 0, 0, 18, 0}; scsi(rs, 6, buf, 18, 1); fprintf(stderr, "TUR status %d sense %x/%02x/%02x\n", s, buf[2]&0xf, buf[12], buf[13]); }
    }
    uint8_t inq[6] = {0x12, 0, 0, 0, 36, 0};
    if (scsi(inq, 6, buf, 36, 1) == 0) fprintf(stderr, "inquiry: %.8s %.16s %.4s\n", buf + 8, buf + 16, buf + 32);
    uint8_t rc[10] = {0x25};
    if (scsi(rc, 10, buf, 8, 1)) { fprintf(stderr, "READ CAPACITY failed (is the Kindle in USB drive mode?)\n"); return 1; }
    nblocks = ((uint64_t)buf[0] << 24 | buf[1] << 16 | buf[2] << 8 | buf[3]) + 1;
    blksz = buf[4] << 24 | buf[5] << 16 | buf[6] << 8 | buf[7];
    fprintf(stderr, "capacity: %llu blocks x %u = %.1f MB\n", (unsigned long long)nblocks, blksz, nblocks * blksz / 1e6);
    if (getenv("USBDISK_EJECT")) { // START STOP UNIT, LoEj=1 Start=0: Kindle leaves USB drive mode
        uint8_t ss[6] = {0x1B, 0, 0, 0, 0x02, 0};
        int r = scsi(ss, 6, NULL, 0, 0);
        fprintf(stderr, "eject: %s\n", r == 0 ? "ok" : "failed");
        return r != 0;
    }
    if (getenv("USBDISK_PROBE")) return 0;
    return fuse_main(argc, argv, &ops, NULL);
}
