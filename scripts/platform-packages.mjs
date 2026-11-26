/**
 * 三平台分包矩阵（Windows x64 / Linux x64 / macOS ARM64）。
 * Release 里放 <packid>-<version>-<suffix>.oplg，另有不带后缀的合包作为回退；
 * 市场侧用 packageMode=1 + 三个 packageUrl* 字段，客户端按平台优先下载。
 */
export const PLATFORM_TARGETS = [
    { id: 'windows-x64', platform: 'windows', arch: 'x64', suffix: 'windows', field: 'packageUrlWindows' },
    { id: 'linux-x64', platform: 'linux', arch: 'x64', suffix: 'linux', field: 'packageUrlLinux' },
    { id: 'macos-arm64', platform: 'macos', arch: 'arm64', suffix: 'macos', field: 'packageUrlMacos' },
];

export function platformPackageName(packid, version, suffix) {
    return `${packid}-${version}-${suffix}.oplg`;
}

export function combinedPackageName(packid, version) {
    return `${packid}-${version}.oplg`;
}

/** 给定发布模式，返回本次应该出现的全部 .oplg 文件名。 */
export function expectedPackageNames(packid, version, platformPackages) {
    return platformPackages
        ? [...PLATFORM_TARGETS.map((target) => platformPackageName(packid, version, target.suffix)), combinedPackageName(packid, version)]
        : [combinedPackageName(packid, version)];
}
