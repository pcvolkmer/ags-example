function debounce(func, timeout = 500) {
    let timer;
    return (...args) => {
        clearTimeout(timer);
        timer = setTimeout(() => { func.apply(this, args); }, timeout);
    }
}

function updateDatalist(value) {
    let url = document.location.origin + document.location.pathname;
    fetch(`${url}?q=${value.trim()}`, { headers: new Headers({ 'Accept': 'application/json' })})
        .then(res => res.json())
        .then(sug => {
            let entriesElem = document.getElementById('entries');
            entriesElem.innerHTML = '';
            sug
                .map(entry => {
                    let option = document.createElement('option');
                    option.value = `${entry['plz']} ${entry['ort']}`;
                    option.innerText = `${entry['plz']} ${entry['ort']}`;
                    return option;
                })
                .forEach(option => {
                    entriesElem.appendChild(option);
                });
        });
}

const updateDatalistDebounced = debounce((value) => updateDatalist(value));

function selectTab(self, elem) {
    Array.from(document.getElementsByClassName('tab')).forEach(e => e.className = 'tab');
    self.className = 'tab active';

    Array.from(document.getElementsByClassName('tabcontent')).forEach(e => e.className = 'tabcontent');
    document.getElementById(elem).className = 'tabcontent active';

    if (elem === 'tab-2' && document.querySelector('#chart canvas') === null) {
        drawMap();
    }
}

function drawMap() {
    let chartElem = document.getElementById('chart');

    if (chartElem === null) {
        return;
    }

    chartElem.innerText = '';
    chartElem.className = 'loading';

    let chart = echarts.init(chartElem);
    let url = document.location.origin + document.location.pathname;
    const urlParams = new URLSearchParams(window.location.search);
    const state = urlParams.get('st');

    Promise.allSettled([
        fetch(`${url}geojson?st=${state}`, { headers: new Headers({ 'Accept': 'application/json' })})
            .then(res => res.json()),
        fetch(`${url}counties_mu_zip?st=${state}`, { headers: new Headers({ 'Accept': 'application/json' })})
            .then(res => res.json())
    ]).then(res => {
        if (res[0].status === 'rejected' || res[1].status === 'rejected') {
            return;
        }

        echarts.registerMap('GEO', res[0].value);
        let data = res[1].value.map(e => {
            if (e.startsWith("02") || e.startsWith("11")) {
                // HH, B
                return {name: e.substring(0, 2), value: 1};
            } else {
                return {name: e, value: 1};
            }
        })

        let option = {
            visualMap: {
                show: false,
                min: 0,
                max: 1,
                inRange: {
                    color: ['transparent', '#d76464']
                },
                calculable: true
            },
            series: [
                {
                    name: 'Kreise',
                    type: 'map',
                    map: 'GEO',
                    // Merkatorprojektion
                    projection: {
                        project: (point) => [point[0] / 180 * Math.PI, -Math.log(Math.tan((Math.PI / 2 + point[1] / 180 * Math.PI) / 2))],
                        unproject: (point) => [point[0] * 180 / Math.PI, 2 * 180 / Math.PI * Math.atan(Math.exp(point[1])) - 90]
                    },
                    emphasis: {
                        label: {
                            backgroundColor: '#777',
                            color: '#fff',
                            position: 'bottom',
                            padding: 4
                        },
                        itemStyle: {
                            areaColor: '#777',
                        }
                    },
                    select: {
                        label: {
                            backgroundColor: '#777',
                            color: '#fff',
                            position: 'bottom',
                            padding: 4
                        },
                        itemStyle: {
                            areaColor: '#777',
                        }
                    },
                    label: {
                        show: false
                    },
                    data: data
                }
            ]
        };

        chartElem.className = '';
        chart.setOption(option);
    });
}