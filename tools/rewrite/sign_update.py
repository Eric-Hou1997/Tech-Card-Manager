#!/usr/bin/env python3
"""Wrap an EXISTING Ed25519 PEM key in the Tauri Minisign wire format.

No key generation, copying, conversion to a secret file, or key-content output.
OpenSSL reads the supplied key itself. Only public keys and signatures leave it.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ITM_PUBLIC='UibeR9KwHBQM0x91qLxz64G0InjT2p0r99a83hLzwo0='
DER_PREFIX=bytes.fromhex('302a300506032b6570032100')

def public_key(private):
    completed=subprocess.run(['openssl','pkey','-in',str(private),'-pubout','-outform','DER'],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    der=completed.stdout
    if len(der)!=44 or not der.startswith(DER_PREFIX):raise ValueError('Existing key must be Ed25519 PKCS8 PEM')
    return der[-32:]

def sign(private,message,directory,name):
    source=directory/(name+'.message');destination=directory/(name+'.signature')
    source.write_bytes(message)
    subprocess.run(['openssl','pkeyutl','-sign','-rawin','-inkey',str(private),'-in',str(source),'-out',str(destination)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
    signature=destination.read_bytes()
    if len(signature)!=64:raise ValueError('Unexpected Ed25519 signature size')
    return signature

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--product',choices=['ITM','TCM'],required=True)
    parser.add_argument('--artifact',type=Path,required=True)
    parser.add_argument('--target',required=True)
    parser.add_argument('--version',required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--expected-public-key',required=True,help='Existing trusted raw Ed25519 public key, base64')
    args=parser.parse_args()
    variable='IMDB_TECH_UPDATE_PRIVATE_KEY' if args.product=='ITM' else 'TECH_CARD_UPDATE_PRIVATE_KEY'
    key_path=os.environ.get(variable)
    if not key_path or not Path(key_path).is_absolute():raise ValueError(variable+' must name the existing private key by absolute path')
    if args.output.exists():raise ValueError('Never overwrite an existing signature output directory')
    key=public_key(Path(key_path));raw_public=base64.b64encode(key).decode()
    if raw_public!=args.expected_public_key or args.product=='ITM' and raw_public!=ITM_PUBLIC:raise ValueError('Private key does not match the existing trusted public key')
    if not args.target.startswith(args.product.lower()+'-'):raise ValueError('Target belongs to another product')
    key_id=hashlib.sha256(key).digest()[:8]
    digest=hashlib.blake2b(digest_size=64);checksum=hashlib.sha256()
    with args.artifact.open('rb') as artifact:
        for block in iter(lambda:artifact.read(1024*1024),b''):digest.update(block);checksum.update(block)
    comment=f'timestamp:{int(time.time())}\tfile:{args.artifact.name}\tproduct:{args.product}\ttarget:{args.target}\tversion:{args.version}'
    if '\n' in comment or '\r' in comment:raise ValueError('Invalid signature metadata')
    with tempfile.TemporaryDirectory(prefix='tauri-public-signature-') as temporary:
        directory=Path(temporary)
        signature=sign(Path(key_path),digest.digest(),directory,'artifact')
        global_signature=sign(Path(key_path),signature+comment.encode(),directory,'metadata')
    public_text='untrusted comment: Existing '+args.product+' Ed25519 public key\n'+base64.b64encode(b'Ed'+key_id+key).decode()+'\n'
    signature_text='untrusted comment: Existing '+args.product+' key; Tauri update\n'+base64.b64encode(b'ED'+key_id+signature).decode()+'\ntrusted comment: '+comment+'\n'+base64.b64encode(global_signature).decode()+'\n'
    args.output.mkdir(parents=True)
    (args.output/(args.artifact.name+'.sig')).write_text(base64.b64encode(signature_text.encode()).decode()+'\n',encoding='ascii')
    (args.output/'tauri-public-key.txt').write_text(base64.b64encode(public_text.encode()).decode()+'\n',encoding='ascii')
    report={'product':args.product,'target':args.target,'version':args.version,'artifact':args.artifact.name,'sha256':checksum.hexdigest(),'raw_public_key':raw_public,'key_replaced':False,'format':'minisign-ED-blake2b-existing-ed25519'}
    (args.output/'signature-metadata.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))

if __name__=='__main__':
    try:main()
    except subprocess.CalledProcessError as error:
        # Do not relay OpenSSL diagnostics that might contain user paths or PEM data.
        raise SystemExit('Existing-key signing failed (OpenSSL exit '+str(error.returncode)+')') from None
