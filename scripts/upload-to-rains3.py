#!/usr/bin/env python3
import os
import sys
import boto3
from pathlib import Path
from botocore.client import Config

# 雨云对象存储配置
S3_ENDPOINT = os.environ.get('RAINS3_ENDPOINT', 'https://cn-nb1.rains3.com')
S3_BUCKET = os.environ.get('RAINS3_BUCKET', 'osirvedio')
S3_ACCESS_KEY = os.environ.get('RAINS3_ACCESS_KEY_ID', '')
S3_SECRET_KEY = os.environ.get('RAINS3_SECRET_ACCESS_KEY', '')
S3_REGION = os.environ.get('RAINS3_REGION', 'cn-nb1')
S3_PREFIX = os.environ.get('RAINS3_PREFIX', 'codex-skins/dreamskin/v1/').strip('/') + '/'

# MIME 类型映射
MIME_TYPES = {
    '.json': 'application/json',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.png': 'image/png',
    '.webp': 'image/webp',
    '.codexskin': 'application/octet-stream'
}

def get_mime_type(file_path):
    ext = Path(file_path).suffix.lower()
    return MIME_TYPES.get(ext, 'application/octet-stream')

def upload_directory(source_dir):
    if not S3_ACCESS_KEY or not S3_SECRET_KEY:
        raise RuntimeError('RAINS3_ACCESS_KEY_ID and RAINS3_SECRET_ACCESS_KEY are required')
    print(f"🚀 开始上传到雨云对象存储...")
    print(f"📂 源目录: {source_dir}")
    print(f"☁️  目标: s3://{S3_BUCKET}/{S3_PREFIX}")
    print()

    # 初始化 S3 客户端
    s3 = boto3.client(
        's3',
        endpoint_url=S3_ENDPOINT,
        aws_access_key_id=S3_ACCESS_KEY,
        aws_secret_access_key=S3_SECRET_KEY,
        region_name=S3_REGION,
        config=Config(signature_version='s3v4')
    )

    # 收集所有文件
    source_path = Path(source_dir)
    files = list(source_path.rglob('*'))
    files = [f for f in files if f.is_file()]

    print(f"📋 找到 {len(files)} 个文件")
    print()

    uploaded = 0
    total_bytes = 0

    for file_path in files:
        try:
            relative_path = file_path.relative_to(source_path)
            s3_key = S3_PREFIX + str(relative_path).replace('\\', '/')

            content_type = get_mime_type(file_path)
            file_size = file_path.stat().st_size

            # 上传文件
            s3.upload_file(
                str(file_path),
                S3_BUCKET,
                s3_key,
                ExtraArgs={
                    'ContentType': content_type,
                    'ACL': 'public-read'
                }
            )

            uploaded += 1
            total_bytes += file_size
            progress = (uploaded / len(files)) * 100

            print(f"[{progress:.1f}%] ✅ {relative_path} ({file_size / 1024:.1f} KB)")

        except Exception as e:
            print(f"❌ 上传失败: {relative_path}")
            print(f"   错误: {e}")

    print()
    print("🎉 上传完成！")
    print(f"📊 统计: {uploaded}/{len(files)} 文件, 总大小 {total_bytes / 1024 / 1024:.2f} MB")
    print()
    print("🌐 访问地址:")
    print(f"   https://osirvedio.cn-nb1.rains3.com/{S3_PREFIX}index.json")

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("用法: python3 upload-to-rains3.py <源目录>")
        sys.exit(1)

    source_dir = sys.argv[1]
    if not os.path.isdir(source_dir):
        print(f"错误: 目录不存在: {source_dir}")
        sys.exit(1)

    upload_directory(source_dir)
