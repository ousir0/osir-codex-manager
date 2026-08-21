#!/usr/bin/env node

const { S3Client, PutObjectCommand } = require('@aws-sdk/client-s3');
const fs = require('fs');
const path = require('path');
const { promisify } = require('util');
const readdir = promisify(fs.readdir);
const stat = promisify(fs.stat);
const readFile = promisify(fs.readFile);

// 雨云对象存储配置
const s3Client = new S3Client({
  endpoint: process.env.RAINS3_ENDPOINT || 'https://cn-nb1.rains3.com',
  region: process.env.RAINS3_REGION || 'cn-nb1',
  credentials: {
    accessKeyId: process.env.RAINS3_ACCESS_KEY_ID || '',
    secretAccessKey: process.env.RAINS3_SECRET_ACCESS_KEY || ''
  },
  forcePathStyle: true
});

const BUCKET = process.env.RAINS3_BUCKET || 'osirvedio';
const SOURCE_DIR = path.join(__dirname, '../dist/dreamskin-community');
const S3_PREFIX = 'codex-skins/dreamskin/v1/';

async function main() {
  console.log('🚀 使用 Python 上传脚本替代...');
  console.log('请运行: python3 scripts/upload-dreamskin-rains3.py dist/dreamskin-community');
}

main();
