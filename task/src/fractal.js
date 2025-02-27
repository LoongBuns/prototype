class Complex {
    constructor(re, im) {
        this.re = re;
        this.im = im;
    }

    static get ONE() { return new Complex(1, 0); }

    add(c) {
        return new Complex(this.re + c.re, this.im + c.im);
    }

    sub(c) {
        return new Complex(this.re - c.re, this.im - c.im);
    }

    mul(c) {
        return new Complex(
            this.re * c.re - this.im * c.im,
            this.re * c.im + this.im * c.re
        );
    }

    div(c) {
        const denom = c.re ** 2 + c.im ** 2;
        return new Complex(
            (this.re * c.re + this.im * c.im) / denom,
            (this.im * c.re - this.re * c.im) / denom
        );
    }

    pow(n) {
        let result = Complex.ONE;
        for (let i = 0; i < n; i++) {
            result = result.mul(this);
        }
        return result;
    }

    abs() {
        return Math.sqrt(this.re ** 2 + this.im ** 2);
    }
}

function hslToRgb(h, s, l) {
    h /= 360; s /= 100; l /= 100;
    let r, g, b;

    if (s === 0) {
        r = g = b = l;
    } else {
        const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
        const p = 2 * l - q;
        r = hue2rgb(p, q, h + 1 / 3);
        g = hue2rgb(p, q, h);
        b = hue2rgb(p, q, h - 1 / 3);
    }

    return [
        Math.round(r * 255),
        Math.round(g * 255),
        Math.round(b * 255)
    ];
}

function hue2rgb(p, q, t) {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
}

export function run(width, height, startY, endY, centerX, zoom, maxIter) {
    const pixelData = new Uint8ClampedArray(width * (endY - startY) * 4);

    for (let y = startY; y < endY; y++) {
        for (let x = 0; x < width; x++) {
            const zx = (x - width / 2) * 4 / (zoom * width) + centerX;
            const zy = (y - height / 2) * 4 / (zoom * height);

            let z = new Complex(zx, zy);
            let iter = 0;
            let root = -1;

            while (iter < maxIter) {
                const f = z.pow(6).sub(Complex.ONE);
                if (f.abs() < 1e-5) {
                    const angle = Math.atan2(z.im, z.re) + Math.PI;
                    root = Math.floor(angle / (Math.PI / 3)) % 6;
                    break;
                }
                const df = z.pow(5).mul(new Complex(6, 0));
                z = z.sub(f.div(df));
                iter++;
            }

            const rgb = hslToRgb(root * 60, 100, 100 - Math.min(iter * 2, 100));
            const idx = ((y - startY) * width + x) * 4;
            pixelData.set([...rgb, 255], idx); // RGBA
        }
    }

    return pixelData;
}