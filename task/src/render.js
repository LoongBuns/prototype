function vec3(x, y, z) {
    return { x: x, y: y, z: z };
}
function add(a, b) {
    return { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z };
}
function sub(a, b) {
    return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z };
}
function scale(v, s) {
    return { x: v.x * s, y: v.y * s, z: v.z * s };
}
function dot(a, b) {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}
function length(v) {
    return Math.sqrt(dot(v, v));
}
function normalize(v) {
    let len = length(v);
    return (len === 0) ? v : scale(v, 1 / len);
}

function Ray(origin, direction) {
    this.origin = origin;
    this.direction = normalize(direction);
}

function Sphere(center, radius, color) {
    this.center = center;
    this.radius = radius;
    this.color = color;
}
Sphere.prototype.intersect = function (ray) {
    let oc = sub(ray.origin, this.center);
    let a = dot(ray.direction, ray.direction);
    let b = 2.0 * dot(oc, ray.direction);
    let c = dot(oc, oc) - this.radius * this.radius;
    let discriminant = b * b - 4 * a * c;
    if (discriminant < 0) return null;
    let t = (-b - Math.sqrt(discriminant)) / (2 * a);
    if (t < 0) t = (-b + Math.sqrt(discriminant)) / (2 * a);
    return (t >= 0) ? t : null;
};

function rayColor(ray, spheres, lightDir) {
    let closest = Infinity;
    let hitSphere = null;
    let tHit = null;
    for (let i = 0; i < spheres.length; i++) {
        let t = spheres[i].intersect(ray);
        if (t !== null && t < closest) {
            closest = t;
            hitSphere = spheres[i];
            tHit = t;
        }
    }
    if (hitSphere) {
        const hitPoint = add(ray.origin, scale(ray.direction, tHit));
        const normal = normalize(sub(hitPoint, hitSphere.center));
        const lightIntensity = Math.max(0, dot(normal, scale(lightDir, -1)));
        return {
            r: Math.min(255, hitSphere.color.r * lightIntensity),
            g: Math.min(255, hitSphere.color.g * lightIntensity),
            b: Math.min(255, hitSphere.color.b * lightIntensity)
        };
    }
    return { r: 135, g: 206, b: 235 };
}

const width = 400;
const height = 300;

const camera = {
    position: { x: 0, y: 0, z: 0 },
    viewportWidth: 2,
    viewportHeight: 1.5,
    focalLength: 1
};

const spheres = [
    { center: { x: 0, y: 0, z: -3 }, radius: 0.5, color: { r: 255, g: 0, b: 0 } },
    { center: { x: 1, y: 0, z: -4 }, radius: 0.5, color: { r: 0, g: 255, b: 0 } },
    { center: { x: -1, y: 0, z: -4 }, radius: 0.5, color: { r: 0, g: 0, b: 255 } }
];

const lightDir = { x: -1, y: -1, z: -1 };
const lightDirLen = Math.sqrt(lightDir.x * lightDir.x + lightDir.y * lightDir.y + lightDir.z * lightDir.z);
lightDir.x /= lightDirLen; lightDir.y /= lightDirLen; lightDir.z /= lightDirLen;

export function run(startRow, endRow) {
    const pixels = [];
    for (let j = startRow; j < endRow; j++) {
        for (let i = 0; i < width; i++) {
            const u = (i + 0.5) / width;
            const v = (j + 0.5) / height;
            const x = (u - 0.5) * camera.viewportWidth;
            const y = (0.5 - v) * camera.viewportHeight;
            const direction = vec3(x, y, -camera.focalLength);
            const ray = new Ray(camera.position, direction);
            const color = rayColor(ray, spheres, lightDir);
            pixels.push(color.r, color.g, color.b, 255);
        }
    }
    return pixels;
}
