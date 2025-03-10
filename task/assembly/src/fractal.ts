class Complex {
    re: f64;
    im: f64;

    constructor(re: f64 = 0.0, im: f64 = 0.0) {
        this.re = re;
        this.im = im;
    }

    static ONE(): Complex {
        return new Complex(1.0, 0.0);
    }

    add(c: Complex): Complex {
        return new Complex(this.re + c.re, this.im + c.im);
    }

    sub(c: Complex): Complex {
        return new Complex(this.re - c.re, this.im - c.im);
    }

    mul(c: Complex): Complex {
        return new Complex(
            this.re * c.re - this.im * c.im,
            this.re * c.im + this.im * c.re
        );
    }

    div(c: Complex): Complex {
        const denom = c.re * c.re + c.im * c.im;
        return new Complex(
            (this.re * c.re + this.im * c.im) / denom,
            (this.im * c.re - this.re * c.im) / denom
        );
    }

    pow(n: i32): Complex {
        let result: Complex = Complex.ONE();
        for (let i: i32 = 0; i < n; i++) {
            result = result.mul(this);
        }
        return result;
    }

    abs(): f64 {
        return Math.hypot(this.re, this.im);
    }
}

function hue2rgb(p: f64, q: f64, t: f64): f64 {
    let tt: f64 = t;
    if (tt < 0.0) tt += 1.0;
    if (tt > 1.0) tt -= 1.0;
    if (tt < 1.0 / 6.0) return p + (q - p) * 6.0 * tt;
    if (tt < 1.0 / 2.0) return q;
    if (tt < 2.0 / 3.0) return p + (q - p) * (2.0 / 3.0 - tt) * 6.0;
    return p;
}

function hslToRgb(h: f64, s: f64, l: f64): Uint8Array {
    const hh: f64 = h / 360.0;
    const ss: f64 = s / 100.0;
    const ll: f64 = l / 100.0;

    let r: f64, g: f64, b: f64;

    if (ss === 0.0) {
        r = g = b = ll;
    } else {
        const q: f64 = ll < 0.5 ? ll * (1.0 + ss) : ll + ss - ll * ss;
        const p: f64 = 2.0 * ll - q;
        r = hue2rgb(p, q, hh + 1.0 / 3.0);
        g = hue2rgb(p, q, hh);
        b = hue2rgb(p, q, hh - 1.0 / 3.0);
    }

    const arr = new Uint8Array(4);
    arr[0] = <u8>Math.round(r * 255.0);
    arr[1] = <u8>Math.round(g * 255.0);
    arr[2] = <u8>Math.round(b * 255.0);
    arr[3] = 255;
    return arr;
}

export function run(
    width: i32,
    height: i32,
    startY: i32,
    endY: i32,
    centerX: f64,
    zoom: f64,
    maxIter: i32
): Uint8ClampedArray {
    const pixelData = new Uint8ClampedArray(width * (endY - startY) * 4);

    for (let y: i32 = startY; y < endY; y++) {
        for (let x: i32 = 0; x < width; x++) {
            const zx: f64 = (x - width / 2.0) * 4.0 / (zoom * f64(width)) + centerX;
            const zy: f64 = (y - height / 2.0) * 4.0 / (zoom * f64(height));

            let z: Complex = new Complex(zx, zy);
            let iter: i32 = 0;
            let root: i32 = -1;

            while (iter < maxIter) {
                const f: Complex = z.pow(6).sub(Complex.ONE());
                if (f.abs() < 1e-5) {
                    const angle: f64 = Math.atan2(z.im, z.re) + Math.PI;
                    root = <i32>Math.floor(angle / (Math.PI / 3.0)) % 6;
                    break;
                }
                const df: Complex = z.pow(5).mul(new Complex(6.0, 0.0));
                z = z.sub(f.div(df));
                iter++;
            }

            const rgb: Uint8Array = hslToRgb(
                f64(root * 60),
                100.0,
                100.0 - f64(Math.min(iter * 2, 100))
            );

            const idx: i32 = ((y - startY) * width + x) * 4;
            pixelData[idx] = rgb[0];
            pixelData[idx + 1] = rgb[1];
            pixelData[idx + 2] = rgb[2];
            pixelData[idx + 3] = 255;
        }
    }

    return pixelData;
}
