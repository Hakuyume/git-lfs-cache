#! /usr/bin/env bash
set -eux

GIT_LFS_CACHE=$(realpath $1)
shift
GIT_LFS_CACHE_OPTIONS=("$@")

cd $(mktemp -d)

export XDG_CONFIG_HOME=$(pwd)/config
mkdir -p ${XDG_CONFIG_HOME}/git
touch ${XDG_CONFIG_HOME}/git/config

curl -L https://github.com/git-lfs/lfs-test-server/releases/download/v0.3.0/Linux.AMD64.gz | tar -xzf -
export LFS_ADMINUSER=admin
export LFS_ADMINPASS=$(base64 /dev/urandom | head -c 16)
./lfs-test-server-linux-amd64/lfs-test-server &
LFS_TEST_SERVER_PID=$!
trap "kill ${LFS_TEST_SERVER_PID}" EXIT

git config --global lfs.url http://localhost:8080
git config --global credential.helper "store --file=$(pwd)/credentials"
git credential-store store --file=$(pwd)/credentials << EOD
protocol=http
host=localhost:8080
username=${LFS_ADMINUSER}
password=${LFS_ADMINPASS}
EOD
git config --list

git init --bare origin

git clone origin foo
cd foo
git lfs install --local
git lfs track '*.bin'
for i in {0..7}
do
    dd if=/dev/urandom of=${i}.bin bs=64k count=1
done
git add *.bin .gitattributes
git config user.name name
git config user.email email
git commit -m commit
git push origin
cd -

git clone origin bar
cd bar
${GIT_LFS_CACHE} install "${GIT_LFS_CACHE_OPTIONS[@]}"
git lfs install --local
git lfs pull
${GIT_LFS_CACHE} stats
cd -

kill ${LFS_TEST_SERVER_PID}
trap - EXIT
sleep 1

git clone origin baz
cd baz
${GIT_LFS_CACHE} install "${GIT_LFS_CACHE_OPTIONS[@]}"
git lfs install --local
git lfs pull
${GIT_LFS_CACHE} stats
cd -
